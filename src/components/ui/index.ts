/**
 * UI 组件库的统一入口。
 *
 * 规则：**新代码只从这里 import 组件**，不要再用 `<select>`、`type="range"`、
 * `type="checkbox"`、`<details>`、`title=""` 这些原生件 —— 它们的外观由系统和
 * 浏览器决定，深浅主题、字号缩放、圆角都不受令牌控制，是「同一个界面里出现
 * 两种语言」的唯一来源。原生元素与替代组件的对照见 `docs/UI-KIT.md`。
 *
 * 少数组件仍按原路径单独 import（`ui/modal`、`ui/menu`、`ui/confirm` 等），
 * 因为它们要么依赖 portal/遮罩、要么是浮层，单独引用更清楚；这里也一并导出，
 * 方便新代码只记一个入口。
 */
export { Button, type ButtonProps, type ButtonVariant, type ButtonSize } from "./button";
export { Field, Input, PasswordInput, Textarea, ReadonlyValue } from "./field";
export { Select, type SelectOption, type SelectProps } from "./select";
export { Slider, type SliderProps } from "./slider";
export { Segmented, Chip, type SegmentedOption } from "./choice";
export { Checkbox, Switch } from "./check";
export { Disclosure } from "./disclosure";
export { Tooltip } from "./tooltip";
export { List, ListRow, ListEmpty, type ListRowProps } from "./list";
export { Modal, type ModalProps } from "./modal";
export { Dropdown, MenuItem, MenuLabel, MenuSep } from "./menu";
export { Confirm } from "./confirm";
export { useToast } from "./toast-context";
