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

use encointer_rpc::Error;
use jsonrpsee::{core::RpcResult, proc_macros::rpc};
use sp_api::{Decode, Encode, ProvideRuntimeApi};
use sp_blockchain::HeaderBackend;
use sp_runtime::traits::Block as BlockT;
use std::sync::Arc;

use encointer_bazaar_rpc_runtime_api::BazaarApi as BazaarRuntimeApi;
use encointer_primitives::{
	bazaar::{Business, BusinessIdentifier, OfferingData},
	communities::CommunityIdentifier,
};

#[rpc(client, server)]
pub trait BazaarApi<BlockHash, AccountId>
where
	AccountId: 'static + Encode + Decode + Send + Sync,
{
	#[method(name = "encointer_bazaarGetBusinesses")]
	fn get_businesses(
		&self,
		cid: CommunityIdentifier,
		at: Option<BlockHash>,
	) -> RpcResult<Vec<Business<AccountId>>>;
	#[method(name = "encointer_bazaarGetOfferings")]
	fn get_offerings(
		&self,
		cid: CommunityIdentifier,
		at: Option<BlockHash>,
	) -> RpcResult<Vec<OfferingData>>;
	#[method(name = "encointer_bazaarGetOfferingsForBusiness")]
	fn get_offerings_for_business(
		&self,
		bid: BusinessIdentifier<AccountId>,
		at: Option<BlockHash>,
	) -> RpcResult<Vec<OfferingData>>;
}

pub struct BazaarRpc<Client, Block, AccountId> {
	client: Arc<Client>,
	_marker: std::marker::PhantomData<(Block, AccountId)>,
}

impl<Client, Block, AccountId> BazaarRpc<Client, Block, AccountId> {
	/// Create new `Bazaar` instance with the given reference to the client.
	pub fn new(client: Arc<Client>) -> Self {
		BazaarRpc { client, _marker: Default::default() }
	}
}

impl<Client, Block, AccountId> BazaarApiServer<<Block as BlockT>::Hash, AccountId>
	for BazaarRpc<Client, Block, AccountId>
where
	AccountId: 'static + Clone + Encode + Decode + Send + Sync,
	Block: BlockT,
	Client: Send + Sync + 'static + ProvideRuntimeApi<Block> + HeaderBackend<Block>,
	Client::Api: BazaarRuntimeApi<Block, AccountId>,
{
	fn get_businesses(
		&self,
		cid: CommunityIdentifier,
		at: Option<<Block as BlockT>::Hash>,
	) -> RpcResult<Vec<Business<AccountId>>> {
		let api = self.client.runtime_api();
		let at = at.unwrap_or_else(|| self.client.info().best_hash);
		Ok(api
			.get_businesses(at, &cid)
			.map_err(|e| Error::Runtime(e.into()))?
			.into_iter()
			.map(|(controller, bd)| Business::new(controller, bd))
			.collect())
	}

	fn get_offerings(
		&self,
		cid: CommunityIdentifier,
		at: Option<<Block as BlockT>::Hash>,
	) -> RpcResult<Vec<OfferingData>> {
		let api = self.client.runtime_api();
		let at = at.unwrap_or_else(|| self.client.info().best_hash);
		Ok(api
			.get_businesses(at, &cid)
			.map_err(|e| Error::Runtime(e.into()))?
			.into_iter()
			.flat_map(|bid| api.get_offerings(at, &BusinessIdentifier::new(cid, bid.0)))
			.flatten()
			.collect())
	}

	fn get_offerings_for_business(
		&self,
		bid: BusinessIdentifier<AccountId>,
		at: Option<<Block as BlockT>::Hash>,
	) -> RpcResult<Vec<OfferingData>> {
		let api = self.client.runtime_api();
		let at = at.unwrap_or_else(|| self.client.info().best_hash);

		Ok(api.get_offerings(at, &bid).map_err(|e| Error::Runtime(e.into()))?)
	}
}
