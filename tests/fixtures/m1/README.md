# M1 合成 fixtures

由 `tests/gen_fixtures.py` 使用 PyArrow 26.0.0 生成，不包含用户数据。

- types.parquet：35 个顶层字段、37 个叶子列、6 行、3 个 row group，覆盖全部目标叶子类型和嵌套、中文、emoji、空字符串、字典、公式样式文本。
- expected.json：由 Python 原始值独立计算的整数/Decimal 预期值。
- no_stats.parquet / all_null.parquet / empty.parquet：区分缺失、全部为 null、零行。
- special_float.parquet：全 NaN 与 ±Inf。
- snappy/gzip/zstd/lz4/brotli：压缩元数据。
- wide.parquet：601 列，验证 IPC 和界面分页。
- encrypted_False/True：真实加密 footer/明文 footer 的列加密样例。测试 KMS 的简单包装仅用于合成数据，不是应用加密功能。
- corrupt/zero：错误输入。

部分 row group 缺失统计的场景由 Rust 测试在真实文件的解码元数据上替换一个 column chunk 生成，验证其只影响对应列。
