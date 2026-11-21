export type FeatureUsage = { steps: readonly string[]; destinations: readonly string[] };

// Entry ids resolve through the same catalog as the navigation. These are
// instructions for the bundled template, never inferred runtime readiness.
export const FEATURE_USAGE: Record<string, FeatureUsage> = {
    'twitch-notices': {
        steps: [
            'Twitch接続で配信者と投稿用アカウントを設定・認証します。雛形の受信はbroadcaster、投稿はmoderatorという認証の保存名を使います。ログイン名ではありません。',
            'Studioで受信アカウント・通知対象・投稿先を確認して保存します。',
            '設定した画面の出力先をOBSへ追加します。初期値はstream-alertsです。対象イベントが届いたとき、チャットと画面の両方を確認します。',
        ],
        destinations: ['twitch', 'outputs', 'history'],
    },
    'random-clip': {
        steps: [
            'Twitch接続で設定・認証し、Studioの受信・投稿先を確認して保存します。雛形の受信はbroadcaster、投稿はmoderatorという認証の保存名を使います。ログイン名ではありません。',
            '設定した画面の出力先をOBSへ追加します。初期値はstream-alertsです。',
            '配信者かモデレーターがチャットで「!rc ログイン名」を送ります。紹介文・映像・音声をそれぞれ確認します。',
        ],
        destinations: ['twitch', 'outputs', 'history'],
    },
    voicevox: {
        steps: [
            '追加結果に「再起動が必要」と出た場合、外部アプリからのHTTP入力は再起動後に使います。下記のStudio試験入力は別経路で、保存・適用済みなら直接試せます。',
            'VOICEVOX ENGINEを起動します。Studioで接続先・話者・読み上げ速度を合わせて保存します。',
            'Studioの「設定箇所」を開き、「この機能へ入力を送る」で短い文章を送ります。この機能のWeb入力ノードから処理します。',
            '読み上げを聞いて確認します。音が出ない場合は、一括停止の状態・出力機器・エンジンの接続先を確認します。',
        ],
        destinations: ['pause', 'snapshot'],
    },
    counter: {
        steps: [
            '外部アプリからHTTP入力を送る場合、追加結果に「再起動が必要」と出ていれば、未保存の編集を保存して対象VACを再起動します。Studioからの試験入力は別経路です。',
            'Studioの「設定箇所」を開き、「この機能へ入力を送る」で送信するたびに、回数が1増えます。',
            'Studioの診断欄で「ログノードの出力」を開き、「ログを再取得」で回数を確認します。チャットや配信画面へは送信しません。',
            'Studioで入力や出力を変更して使います。再読込すると回数は0へ戻ります。',
        ],
        destinations: [],
    },
};
