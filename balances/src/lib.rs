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

#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, Encode};
use encointer_communities::CommunityIdentifier;
use fixed::{transcendental::exp, types::I64F64};
use frame_support::{
    debug, decl_error, decl_event, decl_module, decl_storage, dispatch::DispatchResult, ensure,
};
use frame_system::{self as frame_system, ensure_signed};
use rstd::convert::TryInto;
use sp_core::RuntimeDebug;
use sp_runtime::traits::StaticLookup;

#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};

mod mock;
mod tests;
#[cfg(test)]
#[macro_use]
extern crate approx;

// We're working with fixpoint here.
pub type BalanceType = I64F64;

// Logger target
const LOG: &str = "encointer";

/// Demurrage rate per block.
/// Assuming 50% demurrage per year and a block time of 5s
/// ```matlab
/// dec2hex(-round(log(0.5)/(3600/5*24*356) * 2^64),32)
/// ```
/// This needs to be negated in the formula!
// FIXME: how to define negative hex literal?
//pub const DemurrageRate: BalanceType = BalanceType::from_bits(0x0000000000000000000001E3F0A8A973_i128);

#[derive(Encode, Decode, Default, RuntimeDebug, Clone, Copy)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
pub struct BalanceEntry<BlockNumber> {
    /// The balance of the account after last manual adjustment
    pub principal: BalanceType,
    /// The time (block height) at which the balance was last adjusted
    pub last_update: BlockNumber,
}

pub trait Trait: frame_system::Trait + encointer_communities::Trait {
    type Event: From<Event<Self>> + Into<<Self as frame_system::Trait>::Event>;
}

decl_storage! {
    trait Store for Module<T: Trait> as EncointerBalances {
        pub TotalIssuance get(fn total_issuance_entry): map hasher(blake2_128_concat) CommunityIdentifier => BalanceEntry<T::BlockNumber>;
        pub Balance get(fn balance_entry): double_map hasher(blake2_128_concat) CommunityIdentifier, hasher(blake2_128_concat) T::AccountId => BalanceEntry<T::BlockNumber>;
        //pub DemurragePerBlock get(fn demurrage_per_block): BalanceType = DemurrageRate;
    }
}

decl_event!(
    pub enum Event<T> where
        <T as frame_system::Trait>::AccountId,
    {
        /// Token transfer success (community_id, from, to, amount)
        Transferred(CommunityIdentifier, AccountId, AccountId, BalanceType),
    }
);

decl_module! {
    pub struct Module<T: Trait> for enum Call where origin: T::Origin {
        fn deposit_event() = default;

        /// Transfer some balance to another account.
        #[weight = 10_000]
        pub fn transfer(
            origin,
            dest: <T::Lookup as StaticLookup>::Source,
            community_id: CommunityIdentifier,
            amount: BalanceType,
        ) -> DispatchResult {
            let from = ensure_signed(origin)?;
            let to = T::Lookup::lookup(dest)?;
            Self::transfer_(community_id, &from, &to, amount)?;

            Self::deposit_event(RawEvent::Transferred(community_id, from, to, amount));
            Ok(())
        }
    }
}

decl_error! {
    /// Error for token module.
    pub enum Error for Module<T: Trait> {
        BalanceTooLow,
        TotalIssuanceOverflow,
    }
}

impl<T: Trait> Module<T> {
    pub fn balance(community_id: CommunityIdentifier, who: &T::AccountId) -> BalanceType {
        Self::balance_entry_updated(community_id, who).principal
    }

    /// get balance and apply demurrage. This is not a noop! It changes state.
    fn balance_entry_updated(
        community_id: CommunityIdentifier,
        who: &T::AccountId,
    ) -> BalanceEntry<T::BlockNumber> {
        let entry = <Balance<T>>::get(community_id, who);
        Self::apply_demurrage(
            entry,
            <encointer_communities::Module<T>>::community_properties(community_id)
                .demurrage_per_block,
        )
    }

    pub fn total_issuance(community_id: CommunityIdentifier) -> BalanceType {
        Self::total_issuance_entry_updated(community_id).principal
    }

    /// get total_issuance and apply demurrage. This is not a noop! It changes state.
    fn total_issuance_entry_updated(
        community_id: CommunityIdentifier,
    ) -> BalanceEntry<T::BlockNumber> {
        let entry = <TotalIssuance<T>>::get(community_id);
        Self::apply_demurrage(
            entry,
            <encointer_communities::Module<T>>::community_properties(community_id)
                .demurrage_per_block,
        )
    }

    /// calculate actual value with demurrage
    fn apply_demurrage(
        entry: BalanceEntry<T::BlockNumber>,
        demurrage: BalanceType,
    ) -> BalanceEntry<T::BlockNumber> {
        let current_block = frame_system::Module::<T>::block_number();
        let elapsed_time_block_number = current_block - entry.last_update;
        let elapsed_time_u32: u32 = elapsed_time_block_number
            .try_into()
            .ok()
            .expect("blockchain will not exceed 2^32 blocks; qed");
        let elapsed_time = BalanceType::from_num(elapsed_time_u32);
        let exponent: BalanceType = -demurrage * elapsed_time;
        let exp_result: BalanceType = exp(exponent).unwrap();
        //.expect("demurrage should never overflow");
        BalanceEntry {
            principal: entry
                .principal
                .checked_mul(exp_result)
                .expect("demurrage should never overflow"),
            last_update: current_block,
        }
    }

    fn transfer_(
        community_id: CommunityIdentifier,
        from: &T::AccountId,
        to: &T::AccountId,
        amount: BalanceType,
    ) -> DispatchResult {
        let mut entry_from = Self::balance_entry_updated(community_id, from);
        ensure!(entry_from.principal >= amount, Error::<T>::BalanceTooLow);
        //FIXME: delete account if it falls below existential deposit
        if from != to {
            let mut entry_to = Self::balance_entry_updated(community_id, to);
            entry_from.principal -= amount;
            entry_to.principal += amount;
            <Balance<T>>::insert(community_id, from, entry_from);
            <Balance<T>>::insert(community_id, to, entry_to);
        } else {
            <Balance<T>>::insert(community_id, from, entry_from);
        }
        Ok(())
    }

    pub fn issue(
        community_id: CommunityIdentifier,
        who: &T::AccountId,
        amount: BalanceType,
    ) -> DispatchResult {
        debug::RuntimeLogger::init();
        let mut entry_who = Self::balance_entry_updated(community_id, who);
        let mut entry_tot = Self::total_issuance_entry_updated(community_id);
        ensure!(
            entry_tot.principal.checked_add(amount).is_some(),
            Error::<T>::TotalIssuanceOverflow,
        );
        entry_who.principal += amount;
        entry_tot.principal += amount;
        <TotalIssuance<T>>::insert(community_id, entry_tot);
        <Balance<T>>::insert(community_id, who, entry_who);
        debug::debug!(target: LOG, "issue {:?} for {:?}", amount, who);
        Ok(())
    }

    pub fn burn(
        community_id: CommunityIdentifier,
        who: &T::AccountId,
        amount: BalanceType,
    ) -> DispatchResult {
        let mut entry_who = Self::balance_entry_updated(community_id, who);
        let mut entry_tot = Self::total_issuance_entry_updated(community_id);
        entry_who.principal = if let Some(res) = entry_who.principal.checked_sub(amount) {
            ensure!(res >= 0, Error::<T>::BalanceTooLow);
            res
        } else {
            return Err(Error::<T>::BalanceTooLow.into());
        };
        entry_tot.principal -= amount;
        //FIXME: delete account if it falls below existential deposit

        <TotalIssuance<T>>::insert(community_id, entry_tot);
        <Balance<T>>::insert(community_id, who, entry_who);
        Ok(())
    }
}
