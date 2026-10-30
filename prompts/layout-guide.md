【这一页默认长什么样】
生成器按内容块顺序产出：页头 → 字段组 → 分区 → 图片。
页头里第一个文本字段是大标题，其余按「标签：值」排列、行内折行，图片字段渲染成证件照；
分区是标题 + 条目；**条目头是一行三段**：第一个字段加粗、中间字段去标签排在后面、
时间为一段单独靠右（原件就是这种印法），条目下的措辞排成列表。
样式全在共用样式表里，你只需要写 class，不用写 font-size 之类的声明。

【四档介入，从轻到重】
1. 什么都不写 —— 生成器负责，零成本。内容量正常、没有特殊要求就用这档。
2. preset —— 换整体疏密：classic / compact（内容多，压到一页）/ dense（原件级，字号与行距都贴着原件）/
   roomy（内容少，透气）
3. parts[blockId] —— 只重写某一块，其余照常生成。想改一段的排法时用这档。
   这一档**只改结构、class、样式，一个字都不能改**：文字和这一块的结构化内容对不上
   会被直接拒绝，并告诉你是哪一块。
4. html —— 整页接管。作品集、或结构化确实表达不了的版面才用。
   一旦设了 html，parts 与 preset 都不再起作用。
   它和 parts 一样**不许改字**：整页的纯文本要等于这一页所有块的文字之和，
   多一句少一句都会被拒。要改字就用内容工具（set_field / update_entry），
   写在 HTML 里的新字进不了数据层，只会让这次写入被拒。

【组件清单】写在 class 里，布局类互斥、修饰类可叠加

布局
  cols-2 / cols-3      多栏流式排布（技能、短要点），内容会自动分配到各栏
  grid-2 / grid-3      两列 / 三列网格，各块独立，不会被流断
  row                  一行两端对齐：左边标题、右边放时间。逐条生效
  stack                条目之间不留空隙，靠虚线断开
  panel                浅底色块，适合把技能、证书独立出来

手写片段（parts）专用，结构化档不支持
  split                左窄右宽两栏：左边放时间地点，右边放内容
                       要求里面正好是「一个左栏元素 + 一个右栏元素」；
                       生成器产出的条目给不出这个形状，role=split 会被忽略

分区标题（加在 section 上）
  sec.plain            去掉下划线
  sec.tight            标题只画到自己脚下，不拉通整行
  sec.band             标题变成浅底色带
  t.split              标题左、说明右（里面放 <small>2020 至今</small>）

分隔
  rule / rule-dash / rule-strong     细线 / 虚线 / 粗线

文本
  lead                 导语，比正文略大
  note                 小字注
  quote                左竖线引文
  mark                 加粗强调（不换底色）
  accent               用强调色

标签与键值
  tags > tag           标签云，每个 tag 是一个圆角小条；tag.solid 是实心
  kv                   行内键值对，适合「语言：英语 四级：通过」这类短项

图片
  fig                  普通图，figcaption 是图注
  fig.wide             通栏图
  fig.thumb            小图，挂在条目里当配图
  fig.round            圆形，适合头像与图标
  fig-row              一排图并排，各占等宽

表格
  table / table.plain  技能矩阵、时间线；plain 去掉行分隔线

页头（加在 header 上，也可以直接用 set_page_layout 的 head 选项）
  head.center          居中，里面的照片顺便变成圆形
  head.bare            不放分隔线
  head.thick           粗分隔线
  head.aside           姓名与联系方式居中、照片贴右上角（原件最常见的排法）
                      用 head 选项选它；自己手写页头会丢掉这套结构

**页头请用 head 选项，不要手写 <h1>。** 手写的页头会绕开 .head .top/.txt/.name/.meta
这套结构：名字居不居得中、照片在不在右上、导出的 Word 对不对得上，全靠它。

【能写的标签】
section header footer div p span h1 h2 h3 h4 h5 h6 strong b em i u small mark br hr
ul ol li dl dt dd figure figcaption img a blockquote table thead tbody tr th td
svg g path circle rect line polyline polygon ellipse text

【不能写】
script、style、iframe、form、input —— 会被剥掉。
CSS 里不能有 position、animation、transition、z-index —— 绝对定位的元素不知道自己
该落在第几页，会压到页脚外面（分页引擎接上之前一律关着）。
transform 只认 translate(...)：缩放在 Word 里没有对应，放开了就是「屏幕上对、导出去错」。

【图片怎么放】
src 写 asset:<id>，id 从 read_person 的材料清单里取。
不要写 data: 或外部网址，会被清洗掉。

【收尾前自查】
- 字数只是粗估：compact 约 950–1150 字、normal 约 700–900、roomy 约 550–700（含标点）。真实容量看「这一页的实测」，冲突时以实测为准。
- 超了就压一档或减少留白；压过头就放回来，实测达标才算排好（区间只作粗估）
- 你改的只是版面：内容一个字都不会因为排版而丢，放手调

【矢量与图表】
曲线、流程图、时间轴、进度条一律用 <svg>（含 g/path/circle/rect/line/polyline/polygon/ellipse/text）。
canvas 会被清洗掉，不要用；也不要写 <script>、事件属性（onclick 等）。
