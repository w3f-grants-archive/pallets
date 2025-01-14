// Copyright (c) 2019 Alain Brenzikofer
// This file is part of Encointer
//
// Encointer is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Encointer is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with Encointer.  If not, see <http://www.gnu.org/licenses/>.

//! Unit tests for the tokens module.

use super::*;
use crate::mock::{EncointerCeremonies, EncointerCommunities, EncointerScheduler, Timestamp};
use encointer_primitives::{
	ceremonies::{InactivityTimeoutType, Reputation},
	communities::{CommunityIdentifier, NominalIncome as NominalIncomeType},
	democracy::{ProposalAction, ProposalActionIdentifier, ProposalState, Tally, Vote},
};
use frame_support::{
	assert_err, assert_ok,
	traits::{OnFinalize, OnInitialize},
};
use frame_system::pallet_prelude::BlockNumberFor;
use mock::{new_test_ext, EncointerDemocracy, RuntimeOrigin, System, TestRuntime};
use sp_runtime::BoundedVec;
use test_utils::{
	helpers::{account_id, add_population, register_test_community},
	*,
};

fn create_cid() -> CommunityIdentifier {
	return register_test_community::<TestRuntime>(None, 0.0, 0.0)
}

fn alice() -> AccountId {
	AccountKeyring::Alice.into()
}

fn bob() -> AccountId {
	AccountKeyring::Bob.into()
}

type BlockNumber = BlockNumberFor<TestRuntime>;

fn advance_n_blocks(n: u64) {
	for _ in 0..n {
		System::set_block_number(System::block_number() + 1);
		System::on_initialize(System::block_number());
	}
}

pub fn set_timestamp(t: u64) {
	let _ = pallet_timestamp::Pallet::<TestRuntime>::set(RuntimeOrigin::none(), t);
}

/// Run until a particular block.
fn run_to_block(n: u64) {
	while System::block_number() < n {
		if System::block_number() > 1 {
			System::on_finalize(System::block_number());
		}
		set_timestamp(GENESIS_TIME + BLOCKTIME * n);
		Timestamp::on_finalize(System::block_number());
		System::set_block_number(System::block_number() + 1);
		System::on_initialize(System::block_number());
	}
}

/// Progress blocks until the phase changes
fn run_to_next_phase() {
	let phase = EncointerScheduler::current_phase();
	let mut blocknr = System::block_number();
	while phase == EncointerScheduler::current_phase() {
		blocknr += 1;
		run_to_block(blocknr);
	}
}

#[test]
fn proposal_submission_works() {
	new_test_ext().execute_with(|| {
		let cid = create_cid();
		let block = System::block_number();
		let proposal_action =
			ProposalAction::UpdateNominalIncome(cid, NominalIncomeType::from(100u32));

		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice()),
			proposal_action.clone()
		));
		assert_eq!(EncointerDemocracy::proposal_count(), 1);
		let proposal = EncointerDemocracy::proposals(1).unwrap();
		assert_eq!(proposal.state, ProposalState::Ongoing);
		assert_eq!(proposal.action, proposal_action);
		assert_eq!(proposal.start, block);
		assert!(EncointerDemocracy::tallies(1).is_some());
	});
}

#[test]
fn proposal_submission_fails_if_proposal_in_enactment_queue() {
	new_test_ext().execute_with(|| {
		let cid = create_cid();
		let proposal_action =
			ProposalAction::UpdateNominalIncome(cid, NominalIncomeType::from(100u32));

		EnactmentQueue::<TestRuntime>::insert(proposal_action.get_identifier(), 100);

		assert_err!(
			EncointerDemocracy::submit_proposal(
				RuntimeOrigin::signed(alice()),
				proposal_action.clone()
			),
			Error::<TestRuntime>::ProposalWaitingForEnactment
		);
	});
}

#[test]
fn eligible_reputations_works_with_different_reputations() {
	new_test_ext().execute_with(|| {
		let cid = create_cid();
		let cid2 = register_test_community::<TestRuntime>(None, 10.0, 10.0);
		let alice = alice();

		let proposal_action = ProposalAction::SetInactivityTimeout(8);
		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action
		));

		EncointerCeremonies::fake_reputation((cid, 4), &alice, Reputation::Unverified);
		assert!(EncointerDemocracy::eligible_reputations(
			1,
			&alice,
			&BoundedVec::try_from(vec![(cid, 4)]).unwrap(),
		)
		.unwrap()
		.is_empty());

		EncointerCeremonies::fake_reputation((cid, 5), &alice, Reputation::UnverifiedReputable);
		assert!(EncointerDemocracy::eligible_reputations(
			1,
			&alice,
			&BoundedVec::try_from(vec![(cid, 5)]).unwrap(),
		)
		.unwrap()
		.is_empty());

		EncointerCeremonies::fake_reputation((cid2, 4), &alice, Reputation::VerifiedUnlinked);
		assert_eq!(
			EncointerDemocracy::eligible_reputations(
				1,
				&alice,
				&BoundedVec::try_from(vec![(cid2, 4)]).unwrap(),
			)
			.unwrap()
			.len(),
			1
		);

		EncointerCeremonies::fake_reputation((cid2, 3), &alice, Reputation::VerifiedLinked);
		assert_eq!(
			EncointerDemocracy::eligible_reputations(
				1,
				&alice,
				&BoundedVec::try_from(vec![(cid2, 3)]).unwrap(),
			)
			.unwrap()
			.len(),
			1
		);

		let eligible_reputations = EncointerDemocracy::eligible_reputations(
			1,
			&alice,
			&BoundedVec::try_from(vec![(cid, 5), (cid, 4), (cid2, 4), (cid2, 3)]).unwrap(),
		)
		.unwrap();
		assert_eq!(eligible_reputations.len(), 2);

		assert_eq!(eligible_reputations.first().unwrap(), &(cid2, 4u32));

		assert_eq!(eligible_reputations.last().unwrap(), &(cid2, 3u32));
	});
}

#[test]
fn eligible_reputations_works_with_used_reputations() {
	new_test_ext().execute_with(|| {
		let cid = create_cid();
		let alice = alice();

		let proposal_action = ProposalAction::SetInactivityTimeout(8);
		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action
		));

		EncointerCeremonies::fake_reputation((cid, 5), &alice, Reputation::VerifiedLinked);
		// use this reputation for a vote
		VoteEntries::<TestRuntime>::insert(1, (alice.clone(), (cid, 5)), ());

		EncointerCeremonies::fake_reputation((cid, 4), &alice, Reputation::VerifiedLinked);

		let eligible_reputations = EncointerDemocracy::eligible_reputations(
			1,
			&alice,
			&BoundedVec::try_from(vec![(cid, 5), (cid, 4)]).unwrap(),
		)
		.unwrap();
		assert_eq!(eligible_reputations.len(), 1);
		assert_eq!(eligible_reputations.first().unwrap().1, 4);
	});
}

#[test]
fn eligible_reputations_works_with_inexistent_reputations() {
	new_test_ext().execute_with(|| {
		let cid = create_cid();
		let alice = alice();

		let proposal_action = ProposalAction::SetInactivityTimeout(8);
		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action
		));

		EncointerCeremonies::fake_reputation((cid, 4), &alice, Reputation::VerifiedLinked);

		let eligible_reputations = EncointerDemocracy::eligible_reputations(
			1,
			&alice,
			&BoundedVec::try_from(vec![(cid, 4), (cid, 5)]).unwrap(),
		)
		.unwrap();
		assert_eq!(eligible_reputations.len(), 1);
		assert_eq!(eligible_reputations.first().unwrap().1, 4);
	});
}

#[test]
fn eligible_reputations_works_with_cids() {
	new_test_ext().execute_with(|| {
		let cid = create_cid();
		let cid2 = register_test_community::<TestRuntime>(None, 10.0, 10.0);
		let alice = alice();

		let proposal_action =
			ProposalAction::UpdateNominalIncome(cid, NominalIncomeType::from(100u32));
		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action
		));

		EncointerCeremonies::fake_reputation((cid, 5), &alice, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid2, 5), &alice, Reputation::VerifiedLinked);

		let eligible_reputations = EncointerDemocracy::eligible_reputations(
			1,
			&alice,
			&BoundedVec::try_from(vec![(cid, 5), (cid2, 5)]).unwrap(),
		)
		.unwrap();
		assert_eq!(eligible_reputations.len(), 1);
		assert_eq!(eligible_reputations.first().unwrap(), &(cid, 5u32));
	});
}

#[test]
fn eligible_reputations_fails_with_invalid_cindex() {
	new_test_ext().execute_with(|| {
		let cid = create_cid();
		let alice = alice();

		let proposal_action = ProposalAction::SetInactivityTimeout(8);
		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action
		));

		EncointerCeremonies::fake_reputation((cid, 1), &alice, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid, 4), &alice, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid, 6), &alice, Reputation::VerifiedLinked);

		let eligible_reputations = EncointerDemocracy::eligible_reputations(
			1,
			&alice,
			&BoundedVec::try_from(vec![(cid, 1), (cid, 4), (cid, 6)]).unwrap(),
		)
		.unwrap();
		assert_eq!(eligible_reputations.len(), 1);
		assert_eq!(eligible_reputations.first().unwrap(), &(cid, 4u32));
	});
}

#[test]
fn voting_works() {
	new_test_ext().execute_with(|| {
		let cid = create_cid();
		let alice = alice();
		let cid2 = register_test_community::<TestRuntime>(None, 10.0, 10.0);

		let proposal_action =
			ProposalAction::SetInactivityTimeout(InactivityTimeoutType::from(100u32));

		EncointerCeremonies::fake_reputation((cid, 3), &alice, Reputation::Unverified);
		EncointerCeremonies::fake_reputation((cid, 4), &alice, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid, 5), &alice, Reputation::VerifiedLinked);

		assert_err!(
			EncointerDemocracy::vote(
				RuntimeOrigin::signed(alice.clone()),
				1,
				Vote::Aye,
				BoundedVec::try_from(vec![(cid, 3), (cid, 4), (cid, 5)]).unwrap()
			),
			Error::<TestRuntime>::InexistentProposal
		);

		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action.clone()
		));

		assert_ok!(EncointerDemocracy::vote(
			RuntimeOrigin::signed(alice.clone()),
			1,
			Vote::Aye,
			BoundedVec::try_from(vec![(cid, 3), (cid, 4), (cid, 5)]).unwrap()
		));

		let mut tally = EncointerDemocracy::tallies(1).unwrap();
		assert_eq!(tally.turnout, 2);
		assert_eq!(tally.ayes, 2);

		EncointerCeremonies::fake_reputation((cid2, 4), &alice, Reputation::Unverified);
		EncointerCeremonies::fake_reputation((cid2, 5), &alice, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid, 2), &alice, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid2, 6), &alice, Reputation::VerifiedLinked);

		assert_ok!(EncointerDemocracy::vote(
			RuntimeOrigin::signed(alice.clone()),
			1,
			Vote::Nay,
			BoundedVec::try_from(vec![
				(cid, 2),  // invalid beacuse out of range
				(cid, 3),  // invalid beacuse already used
				(cid2, 4), // invlaid because unverified
				(cid2, 5), // valid
				(cid2, 6), // invlaid because out of range
				(cid2, 3), // invlalid non-existent
			])
			.unwrap()
		));

		tally = EncointerDemocracy::tallies(1).unwrap();
		assert_eq!(tally.turnout, 3);
		assert_eq!(tally.ayes, 2);
	});
}

#[test]
fn do_update_proposal_state_fails_with_inexistent_proposal() {
	new_test_ext().execute_with(|| {
		assert_err!(
			EncointerDemocracy::do_update_proposal_state(1),
			Error::<TestRuntime>::InexistentProposal
		);
	});
}

#[test]
fn do_update_proposal_state_fails_with_wrong_state() {
	new_test_ext().execute_with(|| {
		let cid = create_cid();
		let proposal: Proposal<BlockNumber> = Proposal {
			start: BlockNumber::from(1u64),
			start_cindex: 1,
			action: ProposalAction::UpdateNominalIncome(cid, NominalIncomeType::from(100u32)),
			state: ProposalState::Cancelled,
		};
		Proposals::<TestRuntime>::insert(1, proposal);

		let proposal2: Proposal<BlockNumber> = Proposal {
			start: BlockNumber::from(1u64),
			start_cindex: 1,
			action: ProposalAction::UpdateNominalIncome(cid, NominalIncomeType::from(100u32)),
			state: ProposalState::Approved,
		};
		Proposals::<TestRuntime>::insert(2, proposal2);

		assert_err!(
			EncointerDemocracy::do_update_proposal_state(1),
			Error::<TestRuntime>::ProposalCannotBeUpdated
		);

		assert_err!(
			EncointerDemocracy::do_update_proposal_state(2),
			Error::<TestRuntime>::ProposalCannotBeUpdated
		);
	});
}

#[test]
fn do_update_proposal_state_works_with_cancelled_proposal() {
	new_test_ext().execute_with(|| {
		let proposal_action = ProposalAction::SetInactivityTimeout(8);

		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice()),
			proposal_action
		));

		CancelledAtBlock::<TestRuntime>::insert(ProposalActionIdentifier::SetInactivityTimeout, 3);

		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Ongoing);

		advance_n_blocks(5);

		assert_ok!(EncointerDemocracy::do_update_proposal_state(1));

		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Cancelled);
	});
}

#[test]
fn do_update_proposal_state_works_with_too_old_proposal() {
	new_test_ext().execute_with(|| {
		let proposal_action = ProposalAction::SetInactivityTimeout(8);

		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice()),
			proposal_action
		));

		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Ongoing);

		advance_n_blocks(40);

		assert_ok!(EncointerDemocracy::do_update_proposal_state(1));
		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Ongoing);

		advance_n_blocks(1);

		assert_ok!(EncointerDemocracy::do_update_proposal_state(1));
		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Cancelled);
	});
}

#[test]
fn do_update_proposal_state_works() {
	new_test_ext().execute_with(|| {
		let proposal_action = ProposalAction::SetInactivityTimeout(8);

		let alice = alice();
		let cid = register_test_community::<TestRuntime>(None, 10.0, 10.0);

		// make sure electorate is 100
		let pairs = add_population(100, 0);
		for p in pairs {
			EncointerCeremonies::fake_reputation(
				(cid, 5),
				&account_id(&p),
				Reputation::VerifiedLinked,
			);
		}

		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice),
			proposal_action
		));

		assert_ok!(EncointerDemocracy::do_update_proposal_state(1));
		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Ongoing);

		// propsal is passing
		Tallies::<TestRuntime>::insert(1, Tally { turnout: 100, ayes: 100 });

		assert_eq!(EncointerDemocracy::do_update_proposal_state(1).unwrap(), false);
		assert_eq!(
			EncointerDemocracy::proposals(1).unwrap().state,
			ProposalState::Confirming { since: 0 }
		);

		// not passing anymore
		Tallies::<TestRuntime>::insert(1, Tally { turnout: 100, ayes: 0 });

		assert_eq!(EncointerDemocracy::do_update_proposal_state(1).unwrap(), false);
		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Ongoing);

		// nothing changes if repeated
		assert_eq!(EncointerDemocracy::do_update_proposal_state(1).unwrap(), false);
		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Ongoing);

		// passing
		Tallies::<TestRuntime>::insert(1, Tally { turnout: 100, ayes: 100 });

		assert_eq!(EncointerDemocracy::do_update_proposal_state(1).unwrap(), false);
		assert_eq!(
			EncointerDemocracy::proposals(1).unwrap().state,
			ProposalState::Confirming { since: 0 }
		);

		assert_eq!(EncointerDemocracy::enactment_queue(proposal_action.get_identifier()), None);
		advance_n_blocks(11);
		// proposal is enacted
		assert_eq!(EncointerDemocracy::do_update_proposal_state(1).unwrap(), true);
		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Approved);
		assert_eq!(
			EncointerDemocracy::enactment_queue(proposal_action.get_identifier()).unwrap(),
			1
		);
	});
}

#[test]
fn update_proposal_state_extrinsic_works() {
	new_test_ext().execute_with(|| {
		let proposal_action = ProposalAction::SetInactivityTimeout(8);

		let alice = alice();
		let cid = register_test_community::<TestRuntime>(None, 10.0, 10.0);

		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action
		));

		EncointerCeremonies::fake_reputation((cid, 3), &alice, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid, 4), &alice, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid, 5), &alice, Reputation::VerifiedLinked);

		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Ongoing);
		// propsal is passing
		Tallies::<TestRuntime>::insert(1, Tally { turnout: 3, ayes: 3 });
		EncointerDemocracy::update_proposal_state(RuntimeOrigin::signed(alice), 1).unwrap();
		assert_eq!(
			EncointerDemocracy::proposals(1).unwrap().state,
			ProposalState::Confirming { since: 0 }
		);
	});
}

#[test]
fn test_get_electorate_works() {
	new_test_ext().execute_with(|| {
		let cid = create_cid();
		let cid2 = register_test_community::<TestRuntime>(None, 10.0, 10.0);
		let alice = alice();
		let bob = bob();

		EncointerCeremonies::fake_reputation((cid, 4), &alice, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid, 5), &alice, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid2, 3), &bob, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid2, 4), &bob, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid2, 5), &bob, Reputation::VerifiedLinked);

		let proposal_action = ProposalAction::SetInactivityTimeout(8);
		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action
		));

		let proposal_action =
			ProposalAction::UpdateNominalIncome(cid, NominalIncomeType::from(100u32));
		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action
		));

		assert_eq!(EncointerDemocracy::get_electorate(1).unwrap(), 5);
		assert_eq!(EncointerDemocracy::get_electorate(2).unwrap(), 2);
	});
}

#[test]
fn is_passing_works() {
	new_test_ext().execute_with(|| {
		let alice = alice();
		let cid = register_test_community::<TestRuntime>(None, 10.0, 10.0);

		// electorate is 100
		let pairs = add_population(100, 0);
		for p in pairs {
			EncointerCeremonies::fake_reputation(
				(cid, 5),
				&account_id(&p),
				Reputation::VerifiedLinked,
			);
		}

		let proposal_action = ProposalAction::SetInactivityTimeout(8);
		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action
		));

		// turnout below threshold
		Tallies::<TestRuntime>::insert(1, Tally { turnout: 1, ayes: 1 });
		assert_eq!(EncointerDemocracy::is_passing(1).unwrap(), false);

		// low turnout, 60 % approval
		Tallies::<TestRuntime>::insert(1, Tally { turnout: 10, ayes: 6 });
		assert_eq!(EncointerDemocracy::is_passing(1).unwrap(), false);

		// low turnout 90 % approval
		Tallies::<TestRuntime>::insert(1, Tally { turnout: 10, ayes: 9 });
		assert_eq!(EncointerDemocracy::is_passing(1).unwrap(), true);

		// high turnout, 60 % approval
		Tallies::<TestRuntime>::insert(1, Tally { turnout: 100, ayes: 60 });
		assert_eq!(EncointerDemocracy::is_passing(1).unwrap(), true);

		// high turnout 90 % approval
		Tallies::<TestRuntime>::insert(1, Tally { turnout: 100, ayes: 90 });
		assert_eq!(EncointerDemocracy::is_passing(1).unwrap(), true);
	});
}

#[test]
fn enactment_updates_proposal_metadata_and_enactment_queue() {
	new_test_ext().execute_with(|| {
		let cid = create_cid();
		let alice = alice();

		let proposal_action = ProposalAction::SetInactivityTimeout(8);
		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action
		));

		let proposal_action2 =
			ProposalAction::UpdateNominalIncome(cid, NominalIncomeType::from(100u32));
		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action2
		));

		EnactmentQueue::<TestRuntime>::insert(proposal_action.get_identifier(), 1);
		EnactmentQueue::<TestRuntime>::insert(proposal_action2.get_identifier(), 2);

		run_to_next_phase();
		run_to_next_phase();
		run_to_next_phase();

		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Enacted);

		assert_eq!(EncointerDemocracy::proposals(2).unwrap().state, ProposalState::Enacted);

		assert_eq!(EncointerDemocracy::enactment_queue(proposal_action.get_identifier()), None);

		assert_eq!(EncointerDemocracy::enactment_queue(proposal_action2.get_identifier()), None);
	});
}

#[test]
fn proposal_happy_flow() {
	new_test_ext().execute_with(|| {
		let cid = create_cid();
		let cid2 = register_test_community::<TestRuntime>(None, 10.0, 10.0);
		let alice = alice();
		let proposal_action =
			ProposalAction::UpdateNominalIncome(cid, NominalIncomeType::from(13037u32));
		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action
		));

		EncointerCeremonies::fake_reputation((cid, 3), &alice, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid, 4), &alice, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid, 5), &alice, Reputation::VerifiedLinked);
		EncointerCeremonies::fake_reputation((cid2, 3), &alice, Reputation::VerifiedLinked);

		assert_ok!(EncointerDemocracy::vote(
			RuntimeOrigin::signed(alice.clone()),
			1,
			Vote::Aye,
			BoundedVec::try_from(vec![(cid, 3), (cid, 4), (cid, 5),]).unwrap()
		));

		advance_n_blocks(40);
		assert_ok!(EncointerDemocracy::vote(
			RuntimeOrigin::signed(alice.clone()),
			1,
			Vote::Aye,
			BoundedVec::try_from(vec![(cid2, 3)]).unwrap()
		));

		run_to_next_phase();
		run_to_next_phase();
		run_to_next_phase();

		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Enacted);
		assert_eq!(EncointerDemocracy::enactment_queue(proposal_action.get_identifier()), None);
		assert_eq!(EncointerCommunities::nominal_income(cid), NominalIncomeType::from(13037u32));
	});
}

#[test]
fn enact_update_nominal_income_works() {
	new_test_ext().execute_with(|| {
		let cid = create_cid();
		let alice = alice();
		let proposal_action =
			ProposalAction::UpdateNominalIncome(cid, NominalIncomeType::from(13037u32));
		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action
		));

		EnactmentQueue::<TestRuntime>::insert(proposal_action.get_identifier(), 1);

		run_to_next_phase();
		run_to_next_phase();
		run_to_next_phase();

		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Enacted);
		assert_eq!(EncointerDemocracy::enactment_queue(proposal_action.get_identifier()), None);
		assert_eq!(EncointerCommunities::nominal_income(cid), NominalIncomeType::from(13037u32));
	});
}

#[test]
fn enact_set_inactivity_timeout_works() {
	new_test_ext().execute_with(|| {
		let alice = alice();
		let proposal_action =
			ProposalAction::SetInactivityTimeout(InactivityTimeoutType::from(13037u32));
		assert_ok!(EncointerDemocracy::submit_proposal(
			RuntimeOrigin::signed(alice.clone()),
			proposal_action
		));

		EnactmentQueue::<TestRuntime>::insert(proposal_action.get_identifier(), 1);

		run_to_next_phase();
		run_to_next_phase();
		run_to_next_phase();

		assert_eq!(EncointerDemocracy::proposals(1).unwrap().state, ProposalState::Enacted);
		assert_eq!(EncointerDemocracy::enactment_queue(proposal_action.get_identifier()), None);
		assert_eq!(
			EncointerCeremonies::inactivity_timeout(),
			InactivityTimeoutType::from(13037u32)
		);
	});
}
