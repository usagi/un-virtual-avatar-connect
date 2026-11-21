# Flowgraph enum と再利用graph

## user enum

`[[enums]]`はfile内で利用する名前付きvariant集合を宣言する。

```toml
[[enums]]
id = "language"
variants = ["ja", "en"]
```

enum IDとvariantは同一file内で一意。空値と重複はloader errorになる。

## 再利用境界

再利用単位は`[graph].kind = "module"`とtyped `[[graph.inputs]]` / `[[graph.outputs]]`で宣言し、
entry側の`[[instances]]`からgraph-as-nodeとして配置する。`flowgraph.library.input/output`と
`[meta].library_uses`は廃止済み。

正本は[Flowgraph合成設計](./flowgraph-composition.md)を参照。
