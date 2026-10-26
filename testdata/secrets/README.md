# 密钥（不进仓库）

这个目录被 `.gitignore` 排除，只有 `README.md` 会被提交。

## 新建 `ai.local.json`

```json
{
  "apiKey": "sk-..."
}
```

只需要写要覆盖的字段。完整可覆盖项：

```json
{
  "apiKey": "sk-...",
  "baseUrl": "https://tokenhub.tencentmaas.com/v1",
  "model": "minimax-m3.1-flash",
  "temperature": 0.4,
  "reasoning": "off"
}
```

生效范围：

- **开发构建**（`npm run dev` / `tauri dev`）：与 `testdata/ai.json` 合并后作为默认值
- **发行版**：完全不读（`import.meta.env.DEV` 为 false），
  且 `scripts/package.mjs` 发现本目录有 json 会直接拒绝打包

## 为什么要单独一层

测试配置（地址、模型名）应该跟着仓库走，团队里谁都能用；
密钥只属于某个人、某台机器，一旦进了 git 历史就再也删不干净。
