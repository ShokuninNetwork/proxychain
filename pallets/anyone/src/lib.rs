#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://docs.substrate.io/v3/runtime/frame>
pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;


#[frame_support::pallet]
pub mod pallet {
	use sp_runtime::traits::AccountIdConversion;
	use frame_support::{PalletId, dispatch::DispatchResultWithPostInfo, dispatch::{GetDispatchInfo, Dispatchable}, pallet_prelude::{*, StorageValue, ValueQuery}, traits::NamedReservableCurrency, BoundedVec, Blake2_128Concat};
	use frame_system::pallet_prelude::{*, BlockNumberFor};
	use sp_std::prelude::*;


	#[pallet::config]
	pub trait Config: frame_system::Config {
				type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
				#[pallet::constant]
				type PalletId: Get<PalletId>;

				type Callable: Parameter
					+ Dispatchable<RuntimeOrigin = Self::RuntimeOrigin>
					+ GetDispatchInfo;

				type CallableVecMaxLength: Get<u32>;
				type AnyoneIsRoot: Get<bool>;
				type AnyoneSudoDelay: Get<u32>;
				type Currency: NamedReservableCurrency<Self::AccountId, ReserveIdentifier = [u8; 8]> ;
				type SudoDepositExec: Get<u32>;
				type CancelDepositExec: Get<u32>;
				type MaxSudoCalls: Get<u32>;

	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	#[pallet::storage]
	#[pallet::getter(fn sudos)]
	pub(super) type SudoCalls<T: Config> = StorageValue<_, BoundedVec<(BlockNumberFor<T>, BoundedVec<u8, T::CallableVecMaxLength>), T::MaxSudoCalls>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn votes)]
	pub(super) type CallVotes<T: Config> = StorageMap<_, Blake2_128Concat, BoundedVec<u8, T::CallableVecMaxLength>, (u32, u32), ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn user)]
	pub(super) type UserVote<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, BoundedVec<u8, T::CallableVecMaxLength>, OptionQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		AnyoneDid(DispatchResult),
		AnyoneSuDid(DispatchResult),
		AnyoneSuAnnounced,
		AnyoneSuCancelled,
		NothingToCancel
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		ExistingVote,
		InvalidCallLength,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {

		// and we execute them on idle.
		fn on_idle(block_num: BlockNumberFor<T>, remaining_weight: Weight) -> Weight {
			let calls = Self::sudos();
			// calls should be FIFO in all cases
			if let Some(&(wait_until, ref call_encoded)) = calls.clone().get(0){
				if let Ok(call) = T::Callable::decode(&mut &call_encoded.to_vec()[..]){
					let call_weight: Weight = call.get_dispatch_info().weight;
					let res = if call_weight.ref_time() <= remaining_weight.ref_time() && block_num >= wait_until {
						let origin: T::RuntimeOrigin = frame_system::RawOrigin::Root.into();
						let res = call.clone().dispatch(origin);
						Self::deposit_event(Event::AnyoneSuDid(res.map(|_| ()).map_err(|e| e.error)));
						// lazy person's pop_front
						// split_off is guaranteed not to panic because this branch is unreachable for an empty vec.
						SudoCalls::<T>::set(BoundedVec::truncate_from(calls.to_vec().split_off(1)));
						call_weight
					} else {
						//noop
						Weight::zero()
					};
				res
				} else {
					// if call fails to decode, continue without it.
					SudoCalls::<T>::set(BoundedVec::truncate_from(calls.to_vec().split_off(1)));
					Weight::zero()
				}
			} else {
				Weight::zero()
			}
		}
	}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		/// Call `call` from AnyoneOrigin - bypasses all runtime filters! do not use in runtimes with filter based permissioning!
		#[pallet::weight({
			let dispatch_info = call.get_dispatch_info();
			(dispatch_info.weight, dispatch_info.class)
		})]
		pub fn anyone_do(origin: OriginFor<T>, call: Box<<T as Config>::Callable>) -> DispatchResultWithPostInfo {
			// Check that the extrinsic was signed and get the signer.
			// This function will return an error if the extrinsic is not signed.
			// https://docs.substrate.io/v3/runtime/origins
			ensure_signed(origin)?;
			let origin: T::RuntimeOrigin = frame_system::RawOrigin::Signed(T::PalletId::get().into_account_truncating()).into();
			let res = call.dispatch(origin);
			// Emit an event.
			Self::deposit_event(Event::AnyoneDid(res.map(|_| ()).map_err(|e| e.error)));
			// Return a successful DispatchResultWithPostInfo
			Ok(().into())
		}

		/// Call `call` as root! Must be enabled in the runtime. 
		/// Lock up increases any existing lock up. 
		/// When the T::SudoDepositExec threshold is hit for a call between all users,
		/// the call is put into a queue for execution.
		#[pallet::weight({
			let dispatch_info = call.get_dispatch_info();
			(dispatch_info.weight + T::DbWeight::get().reads_writes(4,4), dispatch_info.class)
		})]
		pub fn anyone_sudo(origin: OriginFor<T>, call: Box<<T as Config>::Callable>, lock_up: u32) -> DispatchResultWithPostInfo {
			let signer = ensure_signed(origin)?;
			if let Err(e) = T::Currency::reserve_named(b"__anyone", &signer, lock_up.into()){
				Err(e.into())
			} else {
				let current_supported = Self::user(&signer).and_then(|a_encoded|{
					if let Ok(a) = T::Callable::decode(&mut &a_encoded.to_vec()[..]){
						let current_user_state = Self::votes(BoundedVec::truncate_from(Encode::encode(&a.clone())));
						if current_user_state == (0,0) {
							None
						} else if a == *call.clone() {
							None
						} else {
							Some(Error::<T>::ExistingVote)
						}
					} else {
						Some(Error::<T>::InvalidCallLength)
					}
				});
				match current_supported {
					None => {
						UserVote::<T>::set(&signer, Some(BoundedVec::truncate_from(Encode::encode(&call.clone()))));
						let (mut current_votes, current_cancels) = Self::votes(BoundedVec::truncate_from(Encode::encode(&call.clone())));
						current_votes += lock_up;
						if current_votes >= T::SudoDepositExec::get() {
							let calls = Self::sudos();
							let mut calls_vec = calls.to_vec();
							calls_vec.push(
								(<frame_system::Pallet<T>>::block_number()+T::AnyoneSudoDelay::get().into(), BoundedVec::truncate_from(Encode::encode(&call.clone())))
							);
							SudoCalls::<T>::set(BoundedVec::truncate_from(calls_vec));
							CallVotes::<T>::remove(BoundedVec::truncate_from(Encode::encode(&call.clone())));
							UserVote::<T>::remove(&signer);
							T::Currency::unreserve_all_named(b"__anyone", &signer);
							Self::deposit_event(Event::AnyoneSuAnnounced);
						} else {
							CallVotes::<T>::set(BoundedVec::truncate_from(Encode::encode(&call.clone())), (current_votes, current_cancels));
						}
						Ok(().into())
					},
					Some(e) => {
						Err(e.into())
					}
				}
			}
		}

		/// Cancel an attempted sudo call.
		/// Calls can be cancelled while they are still gathering support or while they are in the queue.
		/// (if there are duplicates, each independent call must be cancelled individually.)
		#[pallet::weight(Weight::from_ref_time(10_000) + T::DbWeight::get().reads_writes(3,3))]
		pub fn anyone_sudo_cancel(origin: OriginFor<T>, call: Box<<T as Config>::Callable>, lock_up: u32) -> DispatchResultWithPostInfo {
			let signer = ensure_signed(origin)?;
			if let Err(e) = T::Currency::reserve_named(b"__anyone", &signer, lock_up.into()){
				Err(e.into())
			} else {
				let current_supported = Self::user(&signer).and_then(|a|{
					let current_user_state = Self::votes(a.clone());
					if current_user_state == (0,0) {
						None
					} else if a.to_vec() == call.clone().encode() {
						None
					} else {
						Some(a)
					}
				});
				match current_supported {
					None => {
						UserVote::<T>::set(&signer, Some(BoundedVec::truncate_from(Encode::encode(&call.clone()))));
						let (current_votes, mut current_cancels) = Self::votes(BoundedVec::truncate_from(Encode::encode(&call.clone())));
						current_cancels += lock_up;
						if current_cancels >= T::CancelDepositExec::get() {
							// if not being supported at all, look in the queue, else shoot down the vote.
							if current_votes == 0 {
								let calls = Self::sudos();
								let mut calls_vec = calls.to_vec();
								if let Some(index_to_remove) = 
									calls_vec.iter().position(
										|(_, call_compare)| *call_compare.to_vec() == Encode::encode(&call.clone())
									){
									calls_vec.remove(index_to_remove);
									SudoCalls::<T>::set(BoundedVec::truncate_from(calls_vec));
									CallVotes::<T>::remove(BoundedVec::truncate_from(Encode::encode(&call.clone())));
									UserVote::<T>::remove(&signer);
									T::Currency::unreserve_all_named(b"__anyone", &signer);
									Self::deposit_event(Event::AnyoneSuCancelled);
								} else {
									UserVote::<T>::remove(&signer);
									T::Currency::unreserve_all_named(b"__anyone", &signer);
									Self::deposit_event(Event::NothingToCancel);
								}
							} else {
								CallVotes::<T>::remove(BoundedVec::truncate_from(Encode::encode(&call.clone())));
								UserVote::<T>::remove(&signer);
								T::Currency::unreserve_all_named(b"__anyone", &signer);
								Self::deposit_event(Event::AnyoneSuCancelled);
							}
						} else {
							CallVotes::<T>::set(BoundedVec::truncate_from(Encode::encode(&call.clone())), (current_votes, current_cancels));
						}
						Ok(().into())
					},
					Some(_) => {

						Err(Error::<T>::ExistingVote.into())
					}
				}
			}
		}

		/// Clear your personal balance reservations if valid, else do nothing.
		#[pallet::weight(Weight::from_ref_time(10_000) + T::DbWeight::get().reads_writes(2,2))]
		pub fn clear_reserved(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
			let signer = ensure_signed(origin)?;
			if let Some(current_supported) = Self::user(&signer){
				let current_user_state = Self::votes(current_supported.clone());
				if current_user_state == (0,0) {
					UserVote::<T>::remove(&signer);
					T::Currency::unreserve_all_named(b"__anyone", &signer);
				}
			} else {
				T::Currency::unreserve_all_named(b"__anyone", &signer);
			}
			Ok(().into())
		}
	}
}
