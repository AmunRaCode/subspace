//! Consensus related types.

use crate::types::RelayError;
use crate::utils::{RelayCounter, RelayCounterVec, ERR_LABEL};
use codec::{Decode, Encode};
use sc_network_common::sync::message::{BlockAttributes, BlockData, BlockRequest};
use sp_runtime::generic::BlockId;
use sp_runtime::traits::{Block as BlockT, NumberFor};
use sp_runtime::Justifications;
use substrate_prometheus_endpoint::{PrometheusError, Registry};

pub(crate) type BlockHash<Block> = <Block as BlockT>::Hash;
pub(crate) type BlockHeader<Block> = <Block as BlockT>::Header;
pub(crate) type Extrinsic<Block> = <Block as BlockT>::Extrinsic;

/// Initial request for a single block.
#[derive(Encode, Decode)]
pub(crate) struct InitialRequest<Block: BlockT, ProtocolRequest> {
    /// Starting block
    pub(crate) from_block: BlockId<Block>,

    /// Requested block components
    pub(crate) block_attributes: BlockAttributes,

    /// The protocol specific part of the request
    pub(crate) protocol_request: ProtocolRequest,
}

/// Initial response for a single block.
#[derive(Encode, Decode)]
pub(crate) struct InitialResponse<Block: BlockT, ProtocolResponse> {
    ///  Hash of the block being downloaded
    pub(crate) block_hash: BlockHash<Block>,

    /// The partial block, without the extrinsics
    pub(crate) partial_block: PartialBlock<Block>,

    /// The opaque protocol specific part of the response.
    /// This is optional because BlockAttributes::BODY may not be set in
    /// the BlockRequest, in which case we don't need to fetch the
    /// extrinsics
    pub(crate) protocol_response: Option<ProtocolResponse>,
}

/// Request to download multiple/complete blocks, for scenarios like the
/// sync phase. This does not involve the protocol optimizations, as the
/// requesting node is just coming up and may not have the transactions
/// in its pool. So it does not make sense to send response with tx hash,
/// and the full blocks are sent back. This behaves like the default
/// substrate block downloader.
#[derive(Encode, Decode)]
pub(crate) struct FullDownloadRequest<Block: BlockT>(pub(crate) BlockRequest<Block>);

/// Response to the full download request.
#[derive(Encode, Decode)]
pub(crate) struct FullDownloadResponse<Block: BlockT>(pub(crate) Vec<BlockData<Block>>);

/// The message to the server
#[allow(clippy::enum_variant_names)]
#[derive(Encode, Decode)]
pub(crate) enum ServerMessage<Block: BlockT, ProtocolRequest> {
    /// Initial message, to be handled both by the client
    /// and the protocol
    InitialRequest(InitialRequest<Block, ProtocolRequest>),

    /// Message to be handled by the protocol
    ProtocolRequest(ProtocolRequest),

    /// Full download request.
    FullDownloadRequest(FullDownloadRequest<Block>),
}

impl<Block: BlockT, ProtocolRequest> From<ProtocolRequest>
    for ServerMessage<Block, ProtocolRequest>
{
    fn from(inner: ProtocolRequest) -> ServerMessage<Block, ProtocolRequest> {
        ServerMessage::ProtocolRequest(inner)
    }
}

/// The partial block response from the server. It has all the fields
/// except the extrinsics(and some other meta info). The extrinsics
/// are handled by the protocol.
#[derive(Encode, Decode)]
pub(crate) struct PartialBlock<Block: BlockT> {
    pub(crate) parent_hash: BlockHash<Block>,
    pub(crate) block_number: NumberFor<Block>,
    pub(crate) block_header_hash: BlockHash<Block>,
    pub(crate) header: Option<BlockHeader<Block>>,
    pub(crate) indexed_body: Option<Vec<Vec<u8>>>,
    pub(crate) justifications: Option<Justifications>,
}

impl<Block: BlockT> PartialBlock<Block> {
    /// Builds the full block data.
    pub(crate) fn block_data(self, body: Option<Vec<Extrinsic<Block>>>) -> BlockData<Block> {
        BlockData::<Block> {
            hash: self.block_header_hash,
            header: self.header,
            body,
            indexed_body: self.indexed_body,
            receipt: None,
            message_queue: None,
            justification: None,
            justifications: self.justifications,
        }
    }
}

/// Client side metrics.
pub(crate) struct ConsensusClientMetrics {
    pub(crate) requests: RelayCounter,
    pub(crate) failed_requests: RelayCounterVec,
    pub(crate) blocks_downloaded: RelayCounter,
    pub(crate) bytes_downloaded: RelayCounter,
    pub(crate) tx_pool_miss: RelayCounter,
}

impl ConsensusClientMetrics {
    pub(crate) fn new(registry: Option<&Registry>) -> Result<Self, PrometheusError> {
        Ok(Self {
            requests: RelayCounter::new(
                "relay_client_requests",
                "Total number of requests initiated by client",
                registry,
            )?,
            failed_requests: RelayCounterVec::new(
                "relay_client_failed_requests",
                "Number of failed client requests",
                &[ERR_LABEL],
                registry,
            )?,
            blocks_downloaded: RelayCounter::new(
                "relay_client_blocks_downloaded",
                "Number of blocks downloaded by client",
                registry,
            )?,
            bytes_downloaded: RelayCounter::new(
                "relay_client_bytes_downloaded",
                "Number of bytes downloaded by client",
                registry,
            )?,
            tx_pool_miss: RelayCounter::new(
                "relay_client_tx_pool_miss",
                "Number of extrinsics not found in the tx pool",
                registry,
            )?,
        })
    }

    /// Updates the metrics on successful download completion.
    pub(crate) fn on_download<Block: BlockT>(&self, blocks: &[BlockData<Block>]) {
        self.requests.inc();
        if let Ok(blocks) = u64::try_from(blocks.len()) {
            self.blocks_downloaded.inc_by(blocks);
        }
        if let Ok(bytes) = u64::try_from(blocks.encoded_size()) {
            self.bytes_downloaded.inc_by(bytes);
        }
    }

    /// Updates the metrics on failed download.
    pub(crate) fn on_download_fail(&self, err: &RelayError) {
        self.requests.inc();
        self.failed_requests.inc(ERR_LABEL, err.as_ref());
    }
}

/// Server side metrics.
pub(crate) struct ConsensusServerMetrics {
    pub(crate) requests: RelayCounter,
    pub(crate) failed_requests: RelayCounterVec,
}

impl ConsensusServerMetrics {
    pub(crate) fn new(registry: Option<&Registry>) -> Result<Self, PrometheusError> {
        Ok(Self {
            requests: RelayCounter::new(
                "relay_server_requests",
                "Total number of requests processed by the server",
                registry,
            )?,
            failed_requests: RelayCounterVec::new(
                "relay_server_failed_requests",
                "Number of failed server requests",
                &[ERR_LABEL],
                registry,
            )?,
        })
    }

    /// Updates the metrics on a successful request.
    pub(crate) fn on_request(&self) {
        self.requests.inc();
    }

    /// Updates the metrics on a failed request.
    pub(crate) fn on_failed_request(&self, err: &RelayError) {
        self.requests.inc();
        self.failed_requests.inc(ERR_LABEL, err.as_ref());
    }
}
