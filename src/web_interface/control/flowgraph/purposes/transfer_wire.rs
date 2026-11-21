//! Compact upload envelope: UNVACZIP, u32 LE JSON length, metadata JSON, ZIP bytes.
//! Existing JSON byte-array callers remain supported.
use actix_web::{HttpResponse,http::StatusCode};
const MAGIC:&[u8;8]=b"UNVACZIP";
pub(super) fn encode(metadata:&serde_json::Value,archive:&[u8])->anyhow::Result<Vec<u8>> {
    anyhow::ensure!(metadata.is_object()&&metadata.get("archive").is_none(),"invalid envelope metadata");
    let metadata=serde_json::to_vec(metadata)?;
    anyhow::ensure!(metadata.len()<=1024*1024&&!archive.is_empty(),"invalid envelope size");
    let total=12+metadata.len()+archive.len();
    anyhow::ensure!(total<=64*1024*1024,"purpose export exceeds64MiB including metadata");
    let mut wire=Vec::with_capacity(total);wire.extend_from_slice(MAGIC);wire.extend_from_slice(&(metadata.len() as u32).to_le_bytes());wire.extend_from_slice(&metadata);wire.extend_from_slice(archive);Ok(wire)
}
pub(super) fn decode(body:&[u8])->Result<(&[u8],Option<&[u8]>),HttpResponse>{
    let bad=||super::super::util::err_json(StatusCode::BAD_REQUEST,"invalid_purpose_upload","Invalid purpose ZIP envelope");
    if body.len()>64*1024*1024 {return Err(super::super::util::err_json(StatusCode::PAYLOAD_TOO_LARGE,"purpose_upload_too_large","Upload exceeds64MiB"));}
    if !body.starts_with(MAGIC){return Ok((body,None));}
    if body.len()<12{return Err(bad());}
    let length=u32::from_le_bytes(body[8..12].try_into().unwrap()) as usize;
    if length==0||length>1024*1024||length>body.len()-12{return Err(bad());}
    let metadata=&body[12..12+length];let archive=&body[12+length..];
    if archive.is_empty(){return Err(bad());}
    let value:serde_json::Value=serde_json::from_slice(metadata).map_err(|_|bad())?;
    if !value.is_object()||value.get("archive").is_some(){return Err(bad());}
    Ok((metadata,Some(archive)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_preserves_bytes_and_rejects_ambiguous_or_truncated_metadata(){
        let metadata=br#"{"id":"selected"}"#;
        let mut body=MAGIC.to_vec();body.extend_from_slice(&(metadata.len() as u32).to_le_bytes());body.extend_from_slice(metadata);body.extend_from_slice(&[0,255,1,128]);
        let (decoded,archive)=decode(&body).unwrap();assert_eq!(decoded,metadata);assert_eq!(archive.unwrap(),[0,255,1,128]);
        assert_eq!(body.len(),12+metadata.len()+4);
        assert!(decode(&body[..11]).is_err());assert!(decode(&body[..12+metadata.len()]).is_err());
        body[8..12].copy_from_slice(&u32::MAX.to_le_bytes());assert!(decode(&body).is_err());
        let metadata=br#"{"archive":[],"id":"selected"}"#;
        let mut body=MAGIC.to_vec();body.extend_from_slice(&(metadata.len() as u32).to_le_bytes());body.extend_from_slice(metadata);body.push(1);
        assert!(decode(&body).is_err());
        let legacy=br#"{"archive":[0,255]}"#;assert_eq!(decode(legacy).unwrap(),(legacy.as_slice(),None));
        let metadata=serde_json::json!({"name":"共有する用途"});
        let encoded=encode(&metadata,&[0,255,128]).unwrap();let (decoded,archive)=decode(&encoded).unwrap();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(decoded).unwrap(),metadata);assert_eq!(archive.unwrap(),[0,255,128]);
        assert!(encode(&serde_json::json!({"archive":[]}),&[1]).is_err());
    }
}
