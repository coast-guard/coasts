/// Embedded DNS server for resolving `localcoast` to 127.0.0.1.
///
/// Runs on a high port (default 5354) that doesn't require root. After a
/// one-time `coast dns setup` (which creates `/etc/resolver/localcoast`),
/// macOS routes `localcoast` queries here automatically.
use std::net::{Ipv4Addr, SocketAddr};

use tokio::net::UdpSocket;
use tracing::{debug, error, info, warn};

use hickory_proto::op::{Message, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{Name, RData, Record, RecordType};
use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};

const LOCALCOAST_SUFFIX: &str = "localcoast";
const LOOPBACK: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);
const TTL: u32 = 60;

/// Start the DNS server on the given port. Runs until the task is cancelled.
#[allow(clippy::cognitive_complexity)]
pub async fn run_dns_server(port: u16) {
    let addr = SocketAddr::from((LOOPBACK, port));
    let socket = match UdpSocket::bind(addr).await {
        Ok(s) => s,
        Err(e) => {
            warn!(
                port,
                "failed to bind DNS server: {e} (DNS features disabled)"
            );
            return;
        }
    };
    info!(
        port,
        "DNS server listening (resolves *.localcoast -> 127.0.0.1)"
    );

    let mut buf = vec![0u8; 512];
    loop {
        let (len, src) = match socket.recv_from(&mut buf).await {
            Ok(r) => r,
            Err(e) => {
                error!("DNS recv error: {e}");
                continue;
            }
        };

        let response = match handle_query(&buf[..len]) {
            Ok(bytes) => bytes,
            Err(e) => {
                debug!("malformed DNS query from {src}: {e}");
                continue;
            }
        };

        if let Err(e) = socket.send_to(&response, src).await {
            debug!("DNS send error to {src}: {e}");
        }
    }
}

fn handle_query(data: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let request = Message::from_bytes(data)?;
    let id = request.id();
    let op_code = request.op_code();

    if op_code != OpCode::Query {
        let resp = Message::error_msg(id, op_code, ResponseCode::NotImp);
        return Ok(resp.to_bytes()?);
    }

    let queries = request.queries();
    if queries.is_empty() {
        let resp = Message::error_msg(id, op_code, ResponseCode::FormErr);
        return Ok(resp.to_bytes()?);
    }

    let query = &queries[0];
    let name = query.name();
    let qtype = query.query_type();

    let is_match = is_localcoast_name(name);

    let mut response = Message::new();
    response.set_id(id);
    response.set_message_type(MessageType::Response);
    response.set_op_code(OpCode::Query);
    response.set_authoritative(true);
    response.set_recursion_desired(request.recursion_desired());
    response.add_query(query.clone());

    if is_match && qtype == RecordType::A {
        let mut record = Record::from_rdata(name.clone(), TTL, RData::A(A(LOOPBACK)));
        record.set_dns_class(query.query_class());
        response.add_answer(record);
        response.set_response_code(ResponseCode::NoError);
    } else if is_match {
        response.set_response_code(ResponseCode::NoError);
    } else {
        response.set_response_code(ResponseCode::NXDomain);
    }

    Ok(response.to_bytes()?)
}

fn is_localcoast_name(name: &Name) -> bool {
    let s = name.to_ascii().to_lowercase();
    let trimmed = s.trim_end_matches('.');
    trimmed == LOCALCOAST_SUFFIX || trimmed.ends_with(&format!(".{LOCALCOAST_SUFFIX}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hickory_proto::op::Query;
    use hickory_proto::rr::DNSClass;
    use std::str::FromStr;

    fn make_query(name: &str, qtype: RecordType) -> Vec<u8> {
        let mut msg = Message::new();
        msg.set_id(1234);
        msg.set_message_type(MessageType::Query);
        msg.set_op_code(OpCode::Query);
        msg.set_recursion_desired(true);
        let mut q = Query::new();
        q.set_name(Name::from_str(name).unwrap());
        q.set_query_type(qtype);
        q.set_query_class(DNSClass::IN);
        msg.add_query(q);
        msg.to_bytes().unwrap()
    }

    #[test]
    fn test_localcoast_a_record() {
        let query = make_query("localcoast.", RecordType::A);
        let response_bytes = handle_query(&query).unwrap();
        let response = Message::from_bytes(&response_bytes).unwrap();
        assert_eq!(response.response_code(), ResponseCode::NoError);
        assert_eq!(response.answers().len(), 1);
        let answer = &response.answers()[0];
        match answer.data() {
            RData::A(a) => assert_eq!(a.0, Ipv4Addr::new(127, 0, 0, 1)),
            other => panic!("expected A record, got {other:?}"),
        }
    }

    #[test]
    fn test_subdomain_localcoast_resolves() {
        let query = make_query("dev-1.localcoast.", RecordType::A);
        let response_bytes = handle_query(&query).unwrap();
        let response = Message::from_bytes(&response_bytes).unwrap();
        assert_eq!(response.response_code(), ResponseCode::NoError);
        assert_eq!(response.answers().len(), 1);
        match response.answers()[0].data() {
            RData::A(a) => assert_eq!(a.0, Ipv4Addr::new(127, 0, 0, 1)),
            other => panic!("expected A record, got {other:?}"),
        }
    }

    #[test]
    fn test_non_localcoast_nxdomain() {
        let query = make_query("example.com.", RecordType::A);
        let response_bytes = handle_query(&query).unwrap();
        let response = Message::from_bytes(&response_bytes).unwrap();
        assert_eq!(response.response_code(), ResponseCode::NXDomain);
        assert!(response.answers().is_empty());
    }

    #[test]
    fn test_localcoast_aaaa_no_answer() {
        let query = make_query("localcoast.", RecordType::AAAA);
        let response_bytes = handle_query(&query).unwrap();
        let response = Message::from_bytes(&response_bytes).unwrap();
        assert_eq!(response.response_code(), ResponseCode::NoError);
        assert!(response.answers().is_empty());
    }

    #[test]
    fn test_is_localcoast_name() {
        assert!(is_localcoast_name(&Name::from_str("localcoast.").unwrap()));
        assert!(is_localcoast_name(
            &Name::from_str("dev-1.localcoast.").unwrap()
        ));
        assert!(is_localcoast_name(
            &Name::from_str("foo.bar.localcoast.").unwrap()
        ));
        assert!(!is_localcoast_name(
            &Name::from_str("example.com.").unwrap()
        ));
        assert!(!is_localcoast_name(
            &Name::from_str("notlocalcoast.").unwrap()
        ));
    }
}
