import type { FlowgraphSharePlan } from '../types';

/** A sharing closure is not ownership. Only report resolved incoming references. */
export function sharedUsage(selected: FlowgraphSharePlan, all: FlowgraphSharePlan) {
 if (!selected.root_dir || selected.root_dir !== all.root_dir || !selected.source_digests || !all.source_digests)
  throw Error('保存先を照合できません。共有候補を再取得してください。');
 const before=selected.source_digests, after=all.source_digests;
 if(Object.keys(before).length!==Object.keys(after).length || Object.keys(before).some(key=>before[key]!==after[key]))
  throw Error('確認中に構成が変わりました。共有候補を再取得してください。');
 const members=new Set(selected.files);
 const references=[...all.references,...all.package_references,...all.static_references,...all.type_references];
 const incoming=references.filter(r=>!members.has(r.from_file)&&r.target!==null&&members.has(r.target)&&!r.error);
 const unique=new Map(incoming.map(r=>[JSON.stringify([r.from_file,r.target]),{from:r.from_file,target:r.target!}]));
 return { incoming:[...unique.values()], unresolved:references.filter(r=>r.error).length };
}
