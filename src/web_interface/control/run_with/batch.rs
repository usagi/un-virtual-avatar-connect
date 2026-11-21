//! Apply a reviewed group of edits as one configuration transaction.
use std::collections::{BTreeMap, BTreeSet};
use actix_web::{HttpResponse, http::StatusCode};
use serde::Deserialize;
use toml_edit::DocumentMut;
use super::{RunWithDto, RunWithTableDto, mode_actions, toml_ops, util::{build_views, err_response}};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
 pub index: usize,
 pub effective_id: String,
 /// Omitted for a mode-only edit: leave the original run_with item byte-for-byte.
 pub entry: Option<RunWithDto>,
 #[serde(default)]
 pub mode_actions: BTreeMap<String, String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch { #[serde(default)] pub edits: Vec<Edit>, #[serde(default)] pub additions: Vec<RunWithDto> }

pub fn apply(doc: &mut DocumentMut, batch: &Batch) -> Result<(), HttpResponse> {
 if batch.edits.is_empty() && batch.additions.is_empty() { return Err(err_response(StatusCode::BAD_REQUEST,"empty_batch","変更対象がありません。")); }
 let conf: crate::conf::Conf = toml::from_str(&doc.to_string()).map_err(|e|err_response(StatusCode::BAD_REQUEST,"invalid_conf",e))?;
 let before=build_views(&conf.run_with);
 let mut seen=BTreeSet::new();
 for edit in &batch.edits {
  if !seen.insert(edit.index) { return Err(err_response(StatusCode::BAD_REQUEST,"duplicate_target","同じアプリが重複しています。")); }
  if before.get(edit.index).is_none_or(|v|v.effective_id!=edit.effective_id) {
   return Err(err_response(StatusCode::CONFLICT,"changed_target","編集対象が変更されています。再読込して確認してください。"));
  }
  if let Some(entry)=&edit.entry {
   let command=match entry {RunWithDto::Simple(s)=>s,RunWithDto::Table(t)=>&t.command};
   if command.trim().is_empty() {return Err(err_response(StatusCode::BAD_REQUEST,"empty_command","起動コマンドが空です。"));}
  } else if edit.mode_actions.is_empty() {return Err(err_response(StatusCode::BAD_REQUEST,"empty_edit","変更内容がありません。"));}
 }
 // Stage the complete group. Even a late invalid mode leaves the caller's
 // document unchanged; mutate_conf writes only after this returns success.
 let mut staged=doc.clone();
 for edit in &batch.edits {
  if let Some(entry)=&edit.entry {
   toml_ops::run_with_array(&mut staged)?.replace(edit.index,toml_ops::dto_to_value(entry));
   mode_actions::preserve_id(&mut staged,edit.index,&edit.effective_id)?;
   mode_actions::patch(&mut staged,edit.index,entry)?;
  }
  if !edit.mode_actions.is_empty() {
   let mode_patch=RunWithDto::Table(RunWithTableDto{mode_actions:Some(edit.mode_actions.clone()),..Default::default()});
   mode_actions::patch(&mut staged,edit.index,&mode_patch)?;
  }
 }
 for entry in &batch.additions {
  let command=match entry {RunWithDto::Simple(s)=>s,RunWithDto::Table(t)=>&t.command};
  if command.trim().is_empty() {return Err(err_response(StatusCode::BAD_REQUEST,"empty_command","起動コマンドが空です。"));}
  let index=toml_ops::run_with_array(&mut staged)?.len();
  toml_ops::run_with_array(&mut staged)?.push(toml_ops::dto_to_value(entry));
  let updated:crate::conf::Conf=toml::from_str(&staged.to_string()).map_err(|e|err_response(StatusCode::BAD_REQUEST,"invalid_conf",e))?;
  let views=build_views(&updated.run_with);
  if views.iter().take(index).any(|v|v.effective_id==views[index].effective_id) {
   return Err(err_response(StatusCode::BAD_REQUEST,"duplicate_app_id","追加するアプリの連携IDが重複しています。"));
  }
  mode_actions::patch(&mut staged,index,entry)?;
 }
 *doc=staged;
 Ok(())
}

#[cfg(test)]
mod tests {
 use super::*;
 fn fixture()->DocumentMut { "# keep\nrun_with=[{command='one',id='a',minimized=true},{command='two',id='b'}]\n[modes.custom]\ndisplay_name='任意のモード'\n[modes.custom.flowgraph_groups]\nenable=['alerts']\n[modes.custom.managed_apps]\nstart=['a','outside']\n".parse().unwrap() }
 fn batch(value:serde_json::Value)->Batch {serde_json::from_value(value).unwrap()}
 #[test]fn mode_batch_preserves_commands_and_unrelated_policy(){
  let mut doc=fixture();let before=doc["run_with"].to_string();
  apply(&mut doc,&batch(serde_json::json!({"edits":[{"index":0,"effective_id":"a","mode_actions":{"custom":"stop"}},{"index":1,"effective_id":"b","mode_actions":{"custom":"start"}}]}))).unwrap();
  assert_eq!(doc["run_with"].to_string(),before);
  let c:crate::conf::Conf=toml::from_str(&doc.to_string()).unwrap();
  assert_eq!(c.modes["custom"].managed_apps.start,vec!["outside","b"]);assert_eq!(c.modes["custom"].managed_apps.stop,vec!["a"]);assert_eq!(c.modes["custom"].flowgraph_groups.enable,vec!["alerts"]);
 }
 #[test]fn late_invalid_edit_rolls_back_entire_document(){for action in [serde_json::json!({"absent":"start"}),serde_json::json!({"custom":"invalid"})]{let mut doc=fixture();let before=doc.to_string();let changes=batch(serde_json::json!({"edits":[{"index":0,"effective_id":"a","mode_actions":{"custom":"stop"}},{"index":1,"effective_id":"b","mode_actions":action}]}));assert!(apply(&mut doc,&changes).is_err());assert_eq!(doc.to_string(),before);}}
 #[test]fn wrong_or_duplicate_targets_do_not_mutate(){for target in [serde_json::json!({"index":1,"effective_id":"wrong","mode_actions":{"custom":"start"}}),serde_json::json!({"index":0,"effective_id":"a","mode_actions":{"custom":"start"}})]{let mut doc=fixture();let before=doc.to_string();assert!(apply(&mut doc,&batch(serde_json::json!({"edits":[{"index":0,"effective_id":"a","mode_actions":{"custom":"stop"}},target]}))).is_err());assert_eq!(doc.to_string(),before);}}
 #[test]fn app_detail_and_other_mode_edit_commit_together(){let mut doc=fixture();apply(&mut doc,&batch(serde_json::json!({"edits":[{"index":0,"effective_id":"a","entry":{"command":"changed","id":"new-a"},"mode_actions":{"custom":"stop"}},{"index":1,"effective_id":"b","mode_actions":{"custom":"start"}}]}))).unwrap();let c:crate::conf::Conf=toml::from_str(&doc.to_string()).unwrap();assert_eq!(build_views(&c.run_with)[0].effective_id,"new-a");assert_eq!(c.modes["custom"].managed_apps.stop,vec!["new-a"]);}
 #[test]fn additions_and_edits_commit_together(){let mut doc=fixture();apply(&mut doc,&batch(serde_json::json!({"edits":[{"index":0,"effective_id":"a","mode_actions":{"custom":"stop"}}],"additions":[{"command":"new-app","id":"new","start_on_launch":false,"mode_actions":{"custom":"start"}}]}))).unwrap();let c:crate::conf::Conf=toml::from_str(&doc.to_string()).unwrap();assert_eq!(c.run_with.len(),3);assert_eq!(c.modes["custom"].managed_apps.stop,vec!["a"]);assert!(c.modes["custom"].managed_apps.start.contains(&"new".into()));}
 #[test]fn invalid_additions_do_not_commit_edits_or_prior_additions(){for entry in [serde_json::json!({"command":""}),serde_json::json!({"command":"new","id":"a"}),serde_json::json!({"command":"new","mode_actions":{"absent":"start"}})]{let mut doc=fixture();let before=doc.to_string();assert!(apply(&mut doc,&batch(serde_json::json!({"edits":[{"index":0,"effective_id":"a","mode_actions":{"custom":"stop"}}],"additions":[{"command":"valid","id":"valid"},entry]}))).is_err());assert_eq!(doc.to_string(),before);}}

}
