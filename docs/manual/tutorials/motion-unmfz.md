# Tutorial: motion-unmfz

UNVAC v2 の motion 標準形である **VMC -> UNMF -> UNMF/Z** と、UNMF/Z の常時 ingress を確認する。

> ソース: [`flowgraph.example/unmf-vmc-adapter/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/unmf-vmc-adapter/main.flowgraph.toml) + [`flowgraph.example/unmfz-ingress/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/unmfz-ingress/main.flowgraph.toml)

## ねらい

- VMC は互換 input / output adapter として扱い、内部処理は `unmf` socket に寄せる
- `flowgraph.unmf.from_vmc` / `to_vmc` で VMC と UNMF を相互変換する
- `flowgraph.unmfz.publish` / `encode` / `ingress.unmfz` で UNMF/Z data plane に接続する
- UNAA/Z は control plane、UNMF/Z は motion data plane として分ける

## グラフ構造

```text
VMC payload -> motion.vmc_parse -> unmf.from_vmc
            -> unmf.filter / unmf.map
            -> unmfz.publish (dry_run) / unmfz.encode
            -> unmf.to_vmc -> vmc.extract_blendshape
```

常時受信側:

```text
ingress.unmfz -> unmf.extract_blendshape -> log
```

## 実行手順: adapter smoke

`unmf-vmc-adapter` は fixture 用の sample VMC payload を含むため、外部 VMC sender なしで smoke できます。

1. `flowgraph.example/unmf-vmc-adapter/` を `flowgraph/` にコピー
2. VAC 再起動 or Reload
3. <http://127.0.0.1:57000/input/unmf-vmc-adapter> に任意文字列を投稿
4. `unmfz.publish dry-run` の key と、VMC へ戻した `Joy` の値が log に出ることを確認

実運用で VMC UDP を使う場合は `vmc_in` の `properties.bind` を空いているポートへ合わせ、VMC sender からその UDP port へ送ります。

## 実行手順: UNMF/Z ingress

1. `flowgraph.example/unmfz-ingress/` を `flowgraph/` にコピー
2. `properties.base_key_expr` / `topic_mode` / `schema_major` を接続先の UNMF/Z endpoint に合わせる
3. VAC 再起動 or Reload
4. Zenoh 上の UNMF/Z frame を受信すると `Joy` expression value が log に出る

Flowgraph Studio の UNMF/Z endpoint candidate 生成を使う場合、UNAA/Z discovery metadata から base key / topic mode / schema major が入った `flowgraph.ingress.unmfz` subgraph を生成できます。

## 明示IP/PortでLAN接続する

同一PCの自動探索で届かないLAN構成では、接続経路を`conf.toml`へ明示する。UNMF/Z共通の既定TCP portは`47447`。

UNVACからUNAvatarへpublishする場合:

```toml
[unmfz.listen]
enabled = true
ip = "0.0.0.0"
port = 47447
```

UNAvatar側では、UNVACを実行するPCの実IPと同じportを購読接続先に指定する。`0.0.0.0`は待受用であり、接続先には指定しない。

UNMotionやUNVETからUNVACへ受信する場合:

```toml
[[unmfz.connect]]
ip = "192.168.1.20"
port = 47447

[[unmfz.connect]]
ip = "192.168.1.21"
port = 47448
```

複数の接続先は同じZenoh networkへの参加経路であり、接続先ごとに別の論理入力を作らない。Flowgraphは従来どおり`base_key_expr`でデータを選ぶ。UNAA/Z endpoint advertisementはkey等の論理metadataであり、このtransport設定とは分離する。

## 関連ノード

[`flowgraph.motion.vmc_parse`](../node-catalog.md#flowgraph-motion-vmc-parse) / [`flowgraph.unmf.from_vmc`](../node-catalog.md#flowgraph-unmf-from-vmc) / [`flowgraph.unmfz.publish`](../node-catalog.md#flowgraph-unmfz-publish) / [`flowgraph.ingress.unmfz`](../node-catalog.md#flowgraph-ingress-unmfz)
