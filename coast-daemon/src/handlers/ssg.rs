//! Handler for `coast ssg *` requests (non-streaming variants).
//!
//! Phase: ssg-phase-2 handles `Ps`. `Build` is intercepted upstream in
//! `server.rs` so it can stream progress. Everything else returns a
//! structured "not yet implemented (phase N)" error until later phases
//! wire them in.
//!
//! See `coast-ssg/DESIGN.md §7` for the full request surface and §16
//! for the phased plan.

use coast_core::error::{CoastError, Result};
use coast_core::protocol::{SsgRequest, SsgResponse};

pub async fn handle(req: SsgRequest) -> Result<SsgResponse> {
    match req {
        SsgRequest::Ps => coast_ssg::daemon_integration::ps_ssg(),

        SsgRequest::Build { .. } => {
            // `Build` is a streaming variant intercepted upstream in
            // `server.rs::dispatch_request` / the router. Reaching this
            // arm means the router is out of sync with the protocol.
            unreachable!("SsgRequest::Build handled by handle_ssg_build_streaming")
        }

        // Phase 3 verbs.
        SsgRequest::Run
        | SsgRequest::Start
        | SsgRequest::Stop
        | SsgRequest::Restart
        | SsgRequest::Rm { .. }
        | SsgRequest::Logs { .. }
        | SsgRequest::Exec { .. }
        | SsgRequest::Ports => Err(CoastError::state(
            "coast ssg lifecycle verbs are not yet implemented (phase 3)",
        )),

        // Phase 6 verbs.
        SsgRequest::Checkout { .. } | SsgRequest::Uncheckout { .. } => Err(CoastError::state(
            "coast ssg checkout / uncheckout are not yet implemented (phase 6)",
        )),
    }
}
