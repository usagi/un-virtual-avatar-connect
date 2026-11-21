import type { TwitchIrcIngressSummary } from './types';

export function twitchInputPresentation(inputs: TwitchIrcIngressSummary[]) {
  const joined = [...new Set(inputs.filter(input => input.ready)
    .flatMap(input => input.joined_channels))].sort();
  const pending = inputs.filter(input => !input.ready || input.joined_channels.length === 0);
  const pendingChannels = [...new Set(pending.flatMap(input => input.configured_channels))].sort();
  return {
    label: joined.length > 0
      ? `${joined.length}チャンネルを受信中${pending.length > 0 ? ` · ${pending.length}入力が開始待ち` : ''}`
      : inputs.length > 0 ? '開始待ち' : '未使用',
    detail: [
      joined.length > 0 ? joined.map(channel => `#${channel}`).join('、') : '',
      pendingChannels.length > 0 ? `開始待ち: ${pendingChannels.map(channel => `#${channel}`).join('、')}` : '',
    ].filter(Boolean).join(' / ') || (inputs.length > 0 ? '購読先を確認してください' : 'FlowgraphにTwitchチャット入力はありません'),
  };
}
