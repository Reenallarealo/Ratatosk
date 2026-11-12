# iooffbx

iooffbx 将 BrickLink Studio 的 `.io` 模型转换为 FBX，供 Blender 及其他三维软件导入。

程序使用 Rust 编写，编译后为独立的可执行文件。转换在本机完成。

## 项目简介

Studio 模型里的每个面往往是分开的。iooffbx 在转换过程中整理这些几何：焊接重合顶点，让相同零件共享网格，并按颜色与材质归类。材质覆盖普通、透明、电镀、珠光、金属和橡胶。颜色可输出为线性空间，以便在 Blender 中得到一致的明暗。

可以一次转换多个文件。程序提供终端设置界面，用于选择零件库、保存位置和界面语言。

## 准备

- Rust 1.85 或更新版本
- 一份本地 LDraw 零件库。可使用 Studio 自带的库，或从 [LDraw 零件库](https://library.ldraw.org/) 获取

## 编译与运行

```shell
cargo build --release
```

Windows 下的程序位于 `target/release/iooffbx.exe`。将 `.io` 文件拖到程序上即可转换。直接打开程序会进入设置界面，在其中指定零件库路径。

```shell
iooffbx model.io
iooffbx model.io --ldraw-path "路径\ldraw"
```

## 声明

本程序为独立工具，与乐高集团及 BrickLink 无关。

## 许可证

[MIT License](LICENSE)。Copyright (c) 2026 Reenallarealo.
