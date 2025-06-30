// globals.typ
#import "@preview/touying:0.6.1": *
#import themes.university: *
#import "@preview/numbly:0.1.0": numbly
#import "@preview/octique:0.1.0": *


// as well as some utility functions

// 这里可以添加未来可能需要的全局自定义函数或设置
// 例如，定义一些常用的颜色或文本样式

#let GatedDeltaNet = text(weight: "bold", "Gated DeltaNet")
#let Transformer = text(weight: "bold", "Transformer")
#let RNN = text(weight: "bold", "RNN")
#let LSTM = text(weight: "bold", "LSTM")
#let Attention = text(weight: "bold", "Attention")
#let Mamba = text(weight: "bold", "Mamba")
#let Mamba2 = text(weight: "bold", "Mamba2")
#let GLA = text(weight: "bold", "GLA")
#let DeltaNet = text(weight: "bold", "DeltaNet")
#let GatedDeltaNet-H1 = text(weight: "bold", "Gated DeltaNet-H1")
#let GatedDeltaNet-H2 = text(weight: "bold", "Gated DeltaNet-H2")
#let RetNet = text(weight: "bold", "RetNet")

#let set_heading_2_size(it: heading, size: length) = {
    // 使用 match 语句根据标题的级别 (it.level) 来选择不同的大小
  let size = if it.level == 1 {
    20pt // 一级标题（=）设置为 20pt
  } else if it.level == 2 {
    size // 二级标题（==）设置为 16pt
  } else if it.level == 3 {
    20pt // 三级标题（===）设置为 14pt
  } else {
    12pt // 其他级别的标题（如果存在）设置为 12pt
  }

  // 设置标题文本的样式：
  // - size: 使用我们上面根据级别选择的大小
  // - weight: "bold" 表示加粗
  set text(size: size, weight: "bold")

  // 显示标题本身的内容 (it.body)
  it.body

  // 在标题下方添加一些垂直间距，让布局更好看
  v(0.6em) // 0.6em 表示 0.6 倍当前字体大小的垂直空白
}

#let linkto(url, icon: "link") = link(url, box(baseline: 30%, move(dy: -.15em, octique-inline(icon))))