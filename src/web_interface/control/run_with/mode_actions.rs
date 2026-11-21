use super::{RunWithDto, RunWithView, util::{build_views, err_response}};
use crate::conf::Conf;
use actix_web::{HttpResponse, http::StatusCode};
use serde::Serialize;
use toml_edit::{DocumentMut, Array, Item, Value};

#[derive(Debug, Clone, Serialize)]
pub struct ModeActionView { pub id: String, pub label: String, pub action: String }

pub fn definitions(conf: &Conf) -> Vec<ModeActionView> { conf.modes.iter().map(|(id,m)|ModeActionView{id:id.clone(),label:m.display_name.clone().filter(|s|!s.trim().is_empty()).unwrap_or_else(||id.clone()),action:"inherit".into()}).collect() }

pub fn views(conf: &Conf) -> Vec<RunWithView> {
 let mut result=build_views(&conf.run_with);
 for view in &mut result {
  view.modes=conf.modes.iter().map(|(id,m)| {
   let d=&m.managed_apps;
   let action=if d.start.contains(&view.effective_id){"start"}else if d.stop.contains(&view.effective_id){"stop"}else if d.minimize.contains(&view.effective_id){"minimize"}else if d.leave.contains(&view.effective_id){"leave"}else{"inherit"};
   ModeActionView{id:id.clone(),label:m.display_name.clone().unwrap_or_else(||id.clone()),action:action.into()}
  }).collect();
 }
 result
}
fn parse(doc:&DocumentMut)->Result<Conf,HttpResponse>{toml::from_str(&doc.to_string()).map_err(|e|err_response(StatusCode::BAD_REQUEST,"invalid_conf",e))}
// Keep mode references attached to the edited app when its explicit ID changes.
pub fn preserve_id(doc:&mut DocumentMut,index:usize,old:&str)->Result<(),HttpResponse>{
 let c=parse(doc)?;let new=build_views(&c.run_with)[index].effective_id.clone();
 if old==new{return Ok(())}
 if build_views(&c.run_with).iter().enumerate().any(|(i,v)|i!=index&&v.effective_id==new){return Err(err_response(StatusCode::BAD_REQUEST,"duplicate_app_id","連携IDが重複しています。"))}
 for(mode,m) in &c.modes{let d=&m.managed_apps;for(key,values)in [("start",&d.start),("stop",&d.stop),("minimize",&d.minimize),("leave",&d.leave)]{if values.iter().any(|v|v==old){let values:Array=values.iter().map(|v|if v==old{new.as_str()}else{v.as_str()}).collect();doc["modes"][mode]["managed_apps"][key]=Item::Value(Value::Array(values));}}}Ok(())
}
pub fn patch(doc:&mut DocumentMut,index:usize,dto:&RunWithDto)->Result<(),HttpResponse>{
 let RunWithDto::Table(t)=dto else{return Ok(())};let Some(actions)=&t.mode_actions else{return Ok(())};
 let c=parse(doc)?;let id=build_views(&c.run_with)[index].effective_id.clone();
 if c.run_with.iter().enumerate().any(|(i,_)|i!=index&&build_views(&c.run_with)[i].effective_id==id){return Err(err_response(StatusCode::BAD_REQUEST,"duplicate_app_id","連携IDが重複しています。"))}
 for (mode,action) in actions {
  if !c.modes.contains_key(mode)||!matches!(action.as_str(),"inherit"|"start"|"stop"|"minimize"|"leave"){return Err(err_response(StatusCode::BAD_REQUEST,"invalid_mode_action","モードまたは操作が変更されています。再読込して確認してください。"))}
  let d=&c.modes[mode].managed_apps;
  for (key,existing) in [("start",&d.start),("stop",&d.stop),("minimize",&d.minimize),("leave",&d.leave)]{
   let mut values:Array=existing.iter().filter(|s|*s!=&id).map(|s|s.as_str()).collect();
   if action==key{values.push(id.as_str())}
   // Do not add empty tables/fields for untouched implicit defaults.
   if existing.iter().any(|s|s==&id)||action==key{doc["modes"][mode]["managed_apps"][key]=Item::Value(Value::Array(values));}
  }
 }
 Ok(())
}
pub fn remove(doc:&mut DocumentMut,index:usize)->Result<(),HttpResponse>{
 let c=parse(doc)?;let before=build_views(&c.run_with);
 let arr=super::toml_ops::run_with_array(doc)?;
 if index>=arr.len(){return Err(err_response(StatusCode::NOT_FOUND,"index_out_of_range","削除対象がありません。"))}
 arr.remove(index);let after=build_views(&parse(doc)?.run_with);
 for(mode,m)in &c.modes{let d=&m.managed_apps;for(key,values)in [("start",&d.start),("stop",&d.stop),("minimize",&d.minimize),("leave",&d.leave)]{
  let mapped:Vec<String>=values.iter().filter_map(|v|match before.iter().position(|a|&a.effective_id==v){Some(i) if i==index=>None,Some(i)=>Some(after[if i>index{i-1}else{i}].effective_id.clone()),None=>Some(v.clone())}).collect();
  if &mapped!=values{let a:Array=mapped.iter().map(|s|s.as_str()).collect();doc["modes"][mode]["managed_apps"][key]=Item::Value(Value::Array(a));}
 }}Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn patch_preserves_other_apps_groups_and_unknown_names(){
 let mut doc:DocumentMut="run_with = [{command='demo',id='app'},{command='other',id='other'}]\n[modes.custom]\ndisplay_name='長いユーザー定義の名前'\n[modes.custom.managed_apps]\nstart=['app','other']\n[modes.custom.flowgraph_groups]\nenable=['chat']\n".parse().unwrap();
 let dto:RunWithDto=serde_json::from_value(serde_json::json!({"command":"demo","mode_actions":{"custom":"stop"}})).unwrap();patch(&mut doc,0,&dto).unwrap();let c=parse(&doc).ok().unwrap();assert_eq!(c.modes["custom"].managed_apps.start,vec!["other"]);assert_eq!(c.modes["custom"].managed_apps.stop,vec!["app"]);assert_eq!(c.modes["custom"].flowgraph_groups.enable,vec!["chat"]);assert_eq!(views(&c)[0].modes[0].action,"stop");
 }
 #[test]fn removal_keeps_shifted_automatic_id_references_attached(){let mut doc:DocumentMut="run_with=['first','second']\n[modes.custom.managed_apps]\nstart=['run-with-1','run-with-2']\n".parse().unwrap();remove(&mut doc,0).unwrap();let c=parse(&doc).ok().unwrap();assert_eq!(c.modes["custom"].managed_apps.start,vec!["run-with-1"]);assert_eq!(c.run_with.len(),1)}
 #[test]fn renamed_app_keeps_mode_reference(){let mut doc:DocumentMut="run_with=[{command='demo',id='new'}]\n[modes.custom.managed_apps]\nstart=['old']\n".parse().unwrap();preserve_id(&mut doc,0,"old").unwrap();let c=parse(&doc).ok().unwrap();assert_eq!(c.modes["custom"].managed_apps.start,vec!["new"])}
 #[test]fn undefined_mode_is_rejected(){let mut doc:DocumentMut="run_with=[{command='demo',id='app'}]".parse().unwrap();let dto=serde_json::from_value(serde_json::json!({"command":"demo","mode_actions":{"unknown":"start"}})).unwrap();assert!(patch(&mut doc,0,&dto).is_err())}
}
