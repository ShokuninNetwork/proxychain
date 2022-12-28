use crate::{mock::*};
//use frame_support::{assert_ok};

#[test]
fn it_works_for_default_value() {
	new_test_ext().execute_with(|| {
		
		// Dispatch a signed extrinsic.
		//call = ...
		//assert_ok!(TemplateModule::anyone_do(RuntimeOrigin::signed(1), call));
	});
}
