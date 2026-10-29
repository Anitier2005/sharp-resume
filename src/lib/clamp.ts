/** 把值夹在 [min, max]。四个地方各写过一遍，统一到这里 */
export const clamp = (value: number, min: number, max: number) =>
  Math.min(Math.max(value, min), max);
