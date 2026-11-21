import notices from '../../../../flowgraph.example/twitch-event-notices/main.flowgraph.toml?raw';
import clips from '../../../../flowgraph.example/twitch-random-clip/main.flowgraph.toml?raw';
import speech from '../../../../flowgraph.example/tts-voicevox/main.flowgraph.toml?raw';
import counter from './templates/counter.flowgraph.toml?raw';
import { strToU8, zipSync } from 'fflate';
export const FEATURE_PACKS = [
 { id:'twitch-notices', title:'配信イベントを知らせる', description:'チャネポ・フォロー・サブスク・Raidをチャットと画面へ通知。', needs:'Twitch接続: broadcaster / moderator。OBSにはstream-alertsのブラウザー出力を追加。', source:notices },
 { id:'random-clip', title:'クリップで紹介する', description:'配信者・モデレーターの !rc / !randomclip でランダムなクリップを紹介。', needs:'Twitch接続: broadcaster / moderator。OBSのstream-alerts出力と音声確認。', source:clips },
 { id:'voicevox', title:'入力した文章を読み上げる', description:'Web入力をVOICEVOXで合成再生。AIの契約は不要。', needs:'VOICEVOX ENGINE。追加後に入力経路・話者・接続先を設定。', source:speech },
 { id:'counter', title:'カウンターから自動化を始める', description:'Web入力ごとに1を加え、回数を実行トレースへ出力。Studioで入力や出力を自由に変更できます。', needs:'外部サービス不要。「入力を送る」から試せます。再読込で回数は0に戻ります。配信画面への表示は出力の追加が必要。', source:counter },
] as const;
export function featureZip(source:string, purposeName?:string): Uint8Array {
 return zipSync({
  'manifest.flowgraph.toml':strToU8('[fragment]\nschema = "vac-flowgraph-fragment/v2"\nscope = "file"\nexported_at = "2026-10-17T00:00:00Z"\norigin = "UNVAC feature library"\n'),
  'flowgraph/main.flowgraph.toml':strToU8(source),
  ...(purposeName ? {'purpose.json':strToU8(JSON.stringify({schema_version:1,name:purposeName,roots:['main']}))} : {}),
 });
}
