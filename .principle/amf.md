# AMF0 与 AMF3 协议原理

## 一句话概括

AMF 就像**快递单格式**——规定了寄件人、收件人、包裹内容怎么写，AMF0 是传统格式（清晰但冗长），AMF3 是现代格式（紧凑高效）。

## AMF 核心概念

### AMF 是什么？

AMF（Action Message Format）是 Adobe 设计的**二进制序列化格式**，用于 Flash/RTMP 通信。类似于 JSON，但：

| 特性 | JSON | AMF |
|------|------|-----|
| 格式 | 文本 | 二进制 |
| 类型信息 | 需推断 | 显式标记 |
| 引用支持 | 无 | 原生支持 |
| 效率 | 较低 | 高 |

### 比喻：快递标签系统

```
JSON 就像手写快递单：
┌─────────────────────────────────┐
│ {"name": "张三", "age": 30}      │
│                                 │
│ 每次都要写 "name"、"age"         │
│ 收件人要自己猜类型               │
└─────────────────────────────────┘

AMF 就像智能快递标签：
┌─────────────────────────────────┐
│ [字符串标记][长度][name]         │
│ [字符串标记][长度][张三]         │
│ [数字标记][30]                  │
│                                 │
│ 每个值都有类型标签               │
│ 解析器知道怎么处理               │
└─────────────────────────────────┘
```

---

## AMF0：传统格式

### 数据类型标记

AMF0 用一个字节标记数据类型：

| 标记 | 类型 | 比喻 |
|------|------|------|
| 0x00 | Number | 数值包裹 |
| 0x01 | Boolean | 开关包裹 |
| 0x02 | String | 短信包裹（< 65535 字符） |
| 0x03 | Object | 整理箱 |
| 0x05 | Null | 空包裹 |
| 0x06 | Undefined | 未定义包裹 |
| 0x07 | Reference | 引用标签 |
| 0x08 | EcmaArray | 字典箱 |
| 0x0A | StrictArray | 序列箱 |
| 0x0B | Date | 时钟包裹 |
| 0x0C | LongString | 长信包裹（≥ 65535 字符） |

### 编码示例

#### 数字（Number）

```
┌────────┬────────────────────────────────────┐
│ 0x00   │ IEEE 754 双精度浮点数（8字节）       │
│ 1字节  │           8字节                     │
└────────┴────────────────────────────────────┘

示例：3.14159
编码: 00 40 09 21 F9 F0 1B 86 6E
```

#### 字符串（String）

```
┌────────┬──────────────┬─────────────────────┐
│ 0x02   │ 长度（2字节）│ UTF-8 字符串内容     │
│ 1字节  │   2字节      │     N字节           │
└────────┴──────────────┴─────────────────────┘

示例："hello"
编码: 02 00 05 68 65 6C 6C 6F
         ↑  ↑
         │  └─ 长度 = 5
         └─ 字符串标记
```

#### 对象（Object）

```
┌────────┬─────────────────┬──────────────────┬─────┬──────────┐
│ 0x03   │ 属性名（字符串） │ 属性值（AMF值）   │ ... │ 结束标记 │
│        │ （无类型标记）   │                  │     │ 00 00 09│
└────────┴─────────────────┴──────────────────┴─────┴──────────┘

示例：{name: "张三", age: 30}
编码: 03                          ← 对象标记
      00 04 6E 61 6D 65          ← "name"（无标记）
      02 00 06 E5 BC A0 E4 B8 89 ← "张三"
      00 03 61 67 65             ← "age"
      00 40 3E 00 00 00 00 00 00 ← 30
      00 00 09                   ← 结束标记
```

### 引用机制

AMF0 支持**引用**，避免重复传输相同对象：

```
第一个对象：
┌────────┬─────────────────┐
│ 0x03   │ 对象内容...      │  ← 存入引用表，索引=0
└────────┴─────────────────┘

后续引用：
┌────────┬─────────────┐
│ 0x07   │ 引用ID（2字节）│  ← 直接指向索引 0
└────────┴─────────────┘
```

```rust
// decode.rs 中的引用处理
fn decode_reference<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
    let reference_id = reader.read_u16::<BigEndian>()?;
    Ok(self.ref_cache[reference_id as usize].clone())
}
```

---

## AMF3：现代高效格式

### 与 AMF0 的区别

| 特性 | AMF0 | AMF3 |
|------|------|------|
| 类型标记 | 1字节 | 1字节（更多类型） |
| 整数 | 8字节浮点 | 29位变长整数 |
| 字符串 | 每次完整传输 | 引用表去重 |
| 对象 | 无引用表 | 引用表去重 |
| 类型数量 | 16种 | 18种 |

### 变长整数（U29）

AMF3 的精髓在于 **U29 变长编码**，类似 UTF-8 的设计思想：

```
0xxxxxxx                           ← 7 位有效（1字节）
1xxxxxxx 0xxxxxxx                  ← 14 位有效（2字节）
1xxxxxxx 1xxxxxxx 0xxxxxxx         ← 21 位有效（3字节）
1xxxxxxx 1xxxxxxx 1xxxxxxx xxxxxxxx ← 29 位有效（4字节）
```

```rust
// encode.rs 中的 U29 编码
fn write_u29_int<W: Write>(&mut self, writer: &mut W, value: u32) -> Result<usize, io::Error> {
    if value < 0x80 {
        writer.write_u8(value as u8)?;
        Ok(1)
    } else if value < 0x4000 {
        writer.write_u8(((value >> 7) | 0x80) as u8)?;
        writer.write_u8((value & 0x7F) as u8)?;
        Ok(2)
    } else if value < 0x200000 {
        // 3 字节编码...
    } else {
        // 4 字节编码...
    }
}
```

### 引用表机制

AMF3 的核心优化是**引用表**，就像**去重压缩**：

#### 字符串引用表

```
第一次出现 "hello"：
┌──────────────────────────────────┐
│ 0x06 | (长度<<1 | 1) | "hello"   │  ← 存入表，索引=0
└──────────────────────────────────┘

第二次出现 "hello"：
┌──────────────────┐
│ 0x06 | (0 << 1)  │  ← 引用索引 0
└──────────────────┘
        ↑
      只需 1 字节！
```

#### 对象引用表

```rust
// encode.rs 中的引用查找
fn find_object_reference(&self, value: &Amf3Value) -> Option<usize> {
    for (index, cached_value) in self.object_table.iter().enumerate() {
        if self.amf3_values_equal(cached_value, value) {
            return Some(index);
        }
    }
    None
}
```

### AMF3 数据类型

| 标记 | 类型 | 说明 |
|------|------|------|
| 0x00 | Undefined | 未定义 |
| 0x01 | Null | 空值 |
| 0x02 | False | 布尔假 |
| 0x03 | True | 布尔真 |
| 0x04 | Integer | 29位整数 |
| 0x05 | Double | 64位浮点 |
| 0x06 | String | 字符串（引用支持） |
| 0x07 | XMLDoc | XML文档 |
| 0x08 | Date | 日期 |
| 0x09 | Array | 数组（引用支持） |
| 0x0A | Object | 对象（引用支持） |
| 0x0B | XML | XML |
| 0x0C | ByteArray | 字节数组 |
| 0x0D-0x11 | Vector/Dictionary | 高级类型 |

### 编码示例

#### 整数

```
值: 42
编码: 04 54
      ↑  ↑
      │  └─ U29 编码的 42
      └─ 整数标记

值: 1000000（超过 29 位）
编码: 05 40 F4 24 00 00 00 00 00
      ↑  ↑
      │  └─ IEEE 754 双精度
      └─ 用 Double 表示
```

#### 数组

```
[1, 2, 3] 编码：
09          ← 数组标记
07          ← 密集部分长度 = 3，(3<<1|1) = 7
01          ← 空字符串（关联部分结束）
04 01       ← Integer(1)
04 02       ← Integer(2)
04 03       ← Integer(3)
```

#### 对象

```
class Person:
  name: "张三"
  age: 30

编码:
0A          ← 对象标记
0B          ← traits 标记 (动态对象，1个属性)
01          ← 类名长度 0（匿名类）
0B          ← 属性名长度 = 5, "name" 的 U29
6E 61 6D 65 ← "name"
... 其他属性名
01          ← 属性名结束
02 ...      ← name 的值
04 1E       ← age 的值 (30)
```

---

## 递归深度保护

为防止深层嵌套导致栈溢出：

```rust
// decode.rs
const MAX_RECURSION_DEPTH: usize = 256;

pub fn decode<R: Read>(&mut self, reader: &mut R) -> Result<Amf3Value, io::Error> {
    if self.recursion_depth > MAX_RECURSION_DEPTH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Maximum recursion depth exceeded: {}", MAX_RECURSION_DEPTH),
        ));
    }
    self.recursion_depth += 1;
    let result = self.decode_inner(reader);
    self.recursion_depth -= 1;
    result
}
```

---

## 浮点数比较的陷阱

在引用比较中，浮点数需要特殊处理：

```rust
// 错误做法
(A - B).abs() < f64::EPSILON  // 对大数值不准确

// 正确做法
a.to_bits() == b.to_bits()    // 位模式精确比较
```

```rust
// encode.rs 中的正确实现
(Amf3Value::Double(a), Amf3Value::Double(b)) => {
    a.to_bits() == b.to_bits()  // 正确处理 NaN、±0 等
}
```

---

## AMF0 与 AMF3 互操作

在 RTMP 中，AMF0 和 AMF3 可以混合使用：

```rust
// AMF0 中的 AMF3 对象标记
AMF0_ACMPLUS_OBJECT_MARKER: u8 = 0x11;

// decode.rs 中的处理
fn decode_amf3_object<R: Read>(&mut self, reader: &mut R) -> Result<Amf0Value, io::Error> {
    let mut amf3_decoder = Amf3Decoder::new();
    let amf3_value = amf3_decoder.decode(reader)?;
    Ok(Amf0Value::Amf3Object(Self::amf3_to_bytes(&amf3_value)?))
}
```

---

## 性能对比

```
场景：传输对象 {name: "张三", age: 30} 100 次

AMF0:
- 每次完整编码: ~30 字节
- 100 次: ~3000 字节

AMF3:
- 第一次: ~25 字节（加入引用表）
- 后续 99 次: ~5 字节（引用）
- 总计: 25 + 99*5 = 520 字节

压缩率: ~83%
```

---

## 代码导航

| 文件 | 功能 |
|------|------|
| `amf0/mod.rs` | AMF0 类型定义 |
| `amf0/encode.rs` | AMF0 编码器 |
| `amf0/decode.rs` | AMF0 解码器 |
| `amf3/mod.rs` | AMF3 类型定义 |
| `amf3/encode.rs` | AMF3 编码器（含引用表） |
| `amf3/decode.rs` | AMF3 解码器（含引用表） |

## 设计智慧总结

1. **标记先行**：每个值都有类型标记，解析器知道如何处理
2. **引用去重**：AMF3 的引用表大幅减少重复数据
3. **变长编码**：U29 让小整数占用更少空间
4. **向后兼容**：AMF0 可以嵌入 AMF3 数据
5. **安全保护**：递归深度限制防止栈溢出
