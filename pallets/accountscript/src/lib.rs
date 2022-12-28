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
use frame_support::{dispatch::{DispatchResultWithPostInfo, Dispatchable, GetDispatchInfo}, pallet_prelude::*, traits::{LockableCurrency, ExistenceRequirement, WithdrawReasons, Currency}};
	use frame_system::{pallet_prelude::*, RawOrigin};
	use sp_runtime::Saturating;
	use sp_std::prelude::*;
	use sp_std::fmt::Debug;

	#[pallet::config]
	pub trait Config: frame_system::Config + pallet_lottery::Config {
		type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
		type Callable: Parameter
					+ Dispatchable<RuntimeOrigin = Self::RuntimeOrigin>
					+ GetDispatchInfo;
		type Currency: LockableCurrency<Self::AccountId>;
		type MaxBytesPerItem: Get<u32>;
		type MaxBytesTotal: Get<u32>;
		type BlockFeePerByte: Get<u32>;
		type NumberOfBlocks: Saturating
					+ From<<Self as frame_system::Config>::BlockNumber>
					+ From<u32>
					+ Into<<<Self as self::Config>::Currency as Currency<Self::AccountId>>::Balance>;
		type StorageHandler: ScriptStorage;
		//type Opcodes: ForeignOperation<Self::StorageHandler>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	/// The account set as the lottery account of the chain.
	#[pallet::storage]
	#[pallet::getter(fn lottery_manager)]
	pub(super) type LotteryAccount<T: Config> = StorageValue<_, T::AccountId, OptionQuery>;



	/// Each account can store up to MaxBytes bytes in this pallet's storage
	/// accounts store data and lock in a per-block fee rate paid the next time they unlock that data.
	#[pallet::storage]
	#[pallet::getter(fn byte_count)]
	pub(super) type BytesStored<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, (u32, u32, T::BlockNumber), ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn bytes)]
	pub(super) type AccountBytes<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, BoundedVec<u8, T::MaxBytesPerItem>, ValueQuery>;


	/// Key: (AccountId, u8 [index]), Value: (Condition, Outcome)
	#[pallet::storage]
	#[pallet::getter(fn calls)]
	pub(super) type AccountCalls<T: Config> = StorageMap<_, Blake2_128Concat, (T::AccountId, u8), (Option<BoundedVec<u8, T::MaxBytesPerItem>>, Option<BoundedVec<u8, T::MaxBytesPerItem>>), OptionQuery>;


	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/v3/runtime/events-and-errors
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Event documentation should end with an array that provides descriptive names for event
		/// parameters. [something, who]
		SomethingStored(u32, T::AccountId),
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// There is no action at this account index pair
		EmptyIndex,
		/// The deposit required to create a new condition was not available.
		StorageFeeUnavailable,
		/// Storage space would exceed maximum.
		StorageSpaceFilled,
		/// The prerequisite conditional call did not match the onchain prefix.
		ConditionPrefixMismatch,
		/// The outcome call did not match the onchain prefix
		OutcomePrefixMismatch,
		/// The conditional call failed
		ConditionFailed,
		/// The outcome call failed
		OutcomeFailed,
		/// Attempt to address out of storage range
		StorageMiss,
		///
		CallDecodingFailed,
		/// 
		CallFromDataFailed,
	}

	/***
	pub trait ForeignOperation<S: ScriptStorage> {
		type SelectionType: Clone + Encode + Decode + PartialEq + Debug + TypeInfo + MaxEncodedLen;
		type ParameterType: Clone + Encode + Decode + PartialEq + Debug + TypeInfo + MaxEncodedLen;
		fn do_op(operation: Self::SelectionType, param: Self::ParameterType, caller: S::Location) -> Result<Option<Vec<u8>>, ()>;
		fn do_self(&self, param: Self::ParameterType) -> Result<Option<Vec<u8>>, ()>;
	}

	impl<S: ScriptStorage> ForeignOperation<S> for () {
		type SelectionType = ();
		type ParameterType = ();
		fn do_op(_: Self::SelectionType, _: Self::ParameterType, _: S::Location) -> Result<Option<Vec<u8>>,()>{
			Ok(None)
		}
		fn do_self(&self, _: Self::ParameterType) -> Result<Option<Vec<u8>>, ()>{
			Ok(None)
		}
	}

	impl<
		S: ScriptStorage + 
			Clone + 
			Encode + 
			Decode + 
			PartialEq + 
			Debug + 
			TypeInfo + 
			MaxEncodedLen +
			'static, 
		ChainedForeign: ForeignOperation<S> + 
			Clone + 
			Encode + 
			Decode + 
			PartialEq + 
			Debug + 
			TypeInfo + 
			MaxEncodedLen +
			'static
	> ForeignOperation<S> for Operation<S, ChainedForeign> {
		type SelectionType = Operation<S, ChainedForeign>;
		type ParameterType = ChainedForeign::ParameterType;
		fn do_op(op: Self::SelectionType, param: Self::ParameterType, caller: S::Location) -> Result<Option<Vec<u8>>,()>{
			match op.op_type {
				Self::SelectionType::Nop => Ok(None),
				Self::SelectionType::OpThenStorage(oper, index) => {
					if let Ok(result) = Self::do_op(*oper, param, caller){
						if let Some(value) = result {
							let is_err = S::write_to_account(caller, value, index).is_err();
							if is_err {
								Err(())
							} else {
								Ok(None)
							}
						} else {
							Ok(None)
						}
					} else {
						Err(())
					}
				}, 
				Self::SelectionType::OpForeign(foreign, param) => {
					foreign.do_self(param)
				}
			}
		}
		fn do_self(&self, param: Self::ParameterType) -> Result<Option<Vec<u8>>,()>{
			Self::do_op(*self, param, self.caller)
		}
	}

	#[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
	pub struct Operation<S: ScriptStorage, Foreign: ForeignOperation<S>> {
		op_type: OperationType<S, Foreign>,
		caller: S::Location,
	}

	#[derive(Clone, Encode, Decode, PartialEq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
	pub enum OperationType<S: ScriptStorage, Foreign: ForeignOperation<S>> {
		/// Do nothing
		Nop,
		/// Execute another operation, if it returns something, store the result in account storage at index
		OpThenStorage(Box<Operation<S, Foreign>>, u32),
		/// Execute an operation that exists in a foreign implementation
		OpForeign(Foreign, Foreign::ParameterType),
	}
	***/

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(Weight::from_ref_time(10_000) + T::DbWeight::get().writes(1))]
		pub fn define_condition(origin: OriginFor<T>, condition_index: u8, condition_prefix: Option<BoundedVec<u8, T::MaxBytesPerItem>>, outcome_prefix: Option<BoundedVec<u8, T::MaxBytesPerItem>>) -> DispatchResultWithPostInfo {
			let caller = ensure_signed(origin)?;

			if let Err(e) = Self::write_to_condition(caller, condition_index, condition_prefix, outcome_prefix){
				Err(e.into())
			} else {
				Ok(().into())
			}
		}

		#[pallet::weight(Weight::from_ref_time(10_000) + T::DbWeight::get().writes(1))]
		pub fn define_lottery(origin: OriginFor<T>, target: T::AccountId) -> DispatchResultWithPostInfo {
			ensure_root(origin)?;
			LotteryAccount::<T>::set(Some(target));
			Ok(().into())
		}

		#[pallet::weight(Weight::from_ref_time(10_000) + T::DbWeight::get().writes(1))]
		pub fn try_call_from_data(origin: OriginFor<T>, start: u32, end: u32) -> DispatchResultWithPostInfo {
			let caller = ensure_signed(origin)?;
			let account_bytes : Vec<u8> = AccountBytes::<T>::get(&caller).into_inner();
			if let Some(mut call_encoded) = account_bytes.get(start as usize..end as usize){
				if let Ok(maybe_callable) = <T::Callable as Decode>::decode(&mut call_encoded){
					if let Ok(_) = maybe_callable.dispatch(RawOrigin::Signed(caller).into()){
						// todo: add call weight here somehow
						Ok(().into())
					} else {
						Err(Error::<T>::CallFromDataFailed.into())
					}
				} else {
					Err(Error::<T>::CallDecodingFailed.into())
				}
			} else {
				Err(Error::<T>::StorageMiss.into())
			}
		}

		#[pallet::weight(Weight::from_ref_time(10_000) + T::DbWeight::get().reads_writes(1,1))]
		pub fn trigger_condition_simple(origin: OriginFor<T>, target: T::AccountId, condition_index: u8, condition_call: Box<<T as Config>::Callable>, outcome_call: Box<<T as Config>::Callable>) -> DispatchResultWithPostInfo {
			let caller = ensure_signed(origin)?;
			if let Some((condition_option, outcome_option)) = AccountCalls::<T>::get((target.clone(), condition_index)) {
				if let Some(condition) = condition_option {
					let expected_condition_prefix = Encode::encode(&condition.clone());
					let provided_condition_call = Encode::encode(&condition_call.clone());
					let needle = expected_condition_prefix.as_slice();
					let haystack = provided_condition_call.as_slice();
					// we only need to check the first window, since the prefix is only intended to be a *pre*fix
					if haystack.windows(needle.len()).next() == Some(needle) {
						let res = condition_call.dispatch(RawOrigin::Signed(caller).into());
						let out: DispatchResultWithPostInfo = if res.is_err(){
							Err(Error::<T>::ConditionFailed.into())
						} else {
							Ok(().into())
						};
						out
					} else {
						Err(Error::<T>::ConditionPrefixMismatch.into())
					}?;
				};
				if let Some(outcome) = outcome_option {
					let expected_outcome_prefix = Encode::encode(&outcome.clone());
					let provided_outcome_call = Encode::encode(&outcome_call.clone());
					let needle = expected_outcome_prefix.as_slice();
					let haystack = provided_outcome_call.as_slice();
					if haystack.windows(needle.len()).next() == Some(needle) {
						let res = outcome_call.dispatch(RawOrigin::Signed(target).into());
						let out: DispatchResultWithPostInfo = if res.is_err(){
							Err(Error::<T>::OutcomeFailed.into())
						} else {
							Ok(().into())
						};
						out
					} else {
						Err(Error::<T>::OutcomePrefixMismatch.into())
					}?;
				};
				Ok(().into())
			} else {
				Err(Error::<T>::EmptyIndex.into())
			}
		}
	}

	impl<T: Config> pallet_lottery::ValidateCall<T> for Pallet<T> {
		fn validate_call(call: &<T as pallet_lottery::Config>::RuntimeCall) -> bool {

			// lottery should be the 0th conditional of the LotteryAccoount
			// therefore call should be trigger_condition_*
			// target should be LotteryAccount
			// and condition_index should be 0
			if let Some(lottery_account) = LotteryAccount::<T>::get(){
				let encoded_call = call.encode();
				if let Ok(decoded_call) = crate::pallet::Call::<T>::decode(&mut &encoded_call[..]){ //sidenote: wtf is this syntax?
					match decoded_call {
						crate::pallet::Call::<T>::trigger_condition_simple { target, condition_index, .. } => {
							if target == lottery_account && condition_index == 0 {
								true
							} else {
								false
							}
						},
						_ => false,
					}
				} else {
					false
				}
			} else {
				false
			}
		}
	}

	pub trait ScriptStorage {
		type Location;
		type E;
		fn write_to_account(who: Self::Location, data: Vec<u8>, start_index: u32)->Result<(), Self::E>;
		fn calling_location() -> Self::Location; 
	}

	impl<T: Config> ScriptStorage for Pallet<T> {
		type Location = T::AccountId;
		type E = self::Error<T>;
		fn write_to_account(who: T::AccountId, data: Vec<u8>, start_index: u32)->Result<(),self::Error<T>>{
			let account_bytes = AccountBytes::<T>::get(&who);
			let before_len = account_bytes.len();
			if let Some(after_account) = account_bytes.try_mutate(|bs|{
				let end = start_index as usize+data.len();
				let range = start_index as usize..end;
				bs.splice(range, data.clone());
			}){
				let after_len = after_account.len();
				let adding = after_len > before_len;
				if let Err(e) = Self::update_state_balance(who.clone(), before_len.abs_diff(after_len).try_into().expect("data must be a BoundedVec with max len u32::MAX"), adding){
					Err(e)
				} else {
					AccountBytes::<T>::set(who, after_account.clone());
					Ok(())
				}
			} else {
				Err(self::Error::<T>::StorageSpaceFilled)
			}
		}
		fn calling_location() -> T::AccountId {
			todo!();
		}
	}

	impl<T: Config> Pallet<T> {

		fn write_to_condition(who: T::AccountId, index: u8, condition: Option<BoundedVec<u8, T::MaxBytesPerItem>>, outcome: Option<BoundedVec<u8, T::MaxBytesPerItem>>)->Result<(),self::Error<T>>{
			let current = AccountCalls::<T>::get((who.clone(), index.clone()));
			let stored = match (condition.clone(), outcome.clone()) {
    			(None, None) => None,
    			_ => Some((condition, outcome)),
			};
			let before_len = current.encode().len();
			let after_len = stored.encode().len();
			let adding = after_len > before_len;
			if let Err(e) = Self::update_state_balance(who.clone(), before_len.abs_diff(after_len).try_into().expect("data must be a BoundedVec with max len u32::MAX"), adding){
				Err(e)
			} else {
				AccountCalls::<T>::set((who, index), stored);
				Ok(())
			}
		}

		/// update number of bytes stored for an account, 
		/// charge them a per-block fee for the number of blocks since they last updated, at their previously locked-in rate.
		/// minimum charge of 1 block to discourage spamming changes for free.
		fn update_state_balance(who: T::AccountId, delta: u32, adding: bool) -> Result<(),self::Error<T>> {
			let old_tuple = BytesStored::<T>::get(&who);
			let (old_value, old_price, previous_block): (u32, u32, T::BlockNumber) = old_tuple;
			let current_block_number = <frame_system::Pallet<T>>::block_number();
			let block_delta: T::NumberOfBlocks = (current_block_number - previous_block + 1u32.into()).into();
			let to_pay: T::NumberOfBlocks = T::NumberOfBlocks::from(old_value*old_price).saturating_mul(block_delta);
			let balance_to_pay: <<T as self::Config>::Currency as Currency<T::AccountId>>::Balance = to_pay.into();
			if let Ok(paid) = <T as self::Config>::Currency::withdraw(&who, balance_to_pay, WithdrawReasons::except(WithdrawReasons::RESERVE), ExistenceRequirement::KeepAlive){
				<T as self::Config>::Currency::resolve_creating(&pallet_lottery::Pallet::<T>::account_id(), paid);
				let new_value;
				if adding {
					new_value = old_value + delta;
				} else {
					new_value = old_value - delta;
				}
				if new_value < T::MaxBytesTotal::get(){
					BytesStored::<T>::set(&who, (new_value, T::BlockFeePerByte::get(), current_block_number));
					Ok(())
				} else {
					Err(self::Error::<T>::StorageSpaceFilled)
				}
			} else {
				Err(self::Error::<T>::StorageFeeUnavailable)
			}
		}
	}
}
