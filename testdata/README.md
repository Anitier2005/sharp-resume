# 测试数据

这里放**能进仓库**的东西：mock 数据、测试用的模型配置（地址与模型名，不含密钥）。

```
testdata/
  ai.json               测试用模型配置：baseUrl / model / 参数。可公开
  mock/                 mock 数据：各版本的工作区样例，用来验证迁移
    workspace-v1.json   最早期结构（经历库存 bullets、页面只有 experienceIds）
    workspace-v3.json   会话归到人、措辞在页面 entries 上
  secrets/              密钥，**不进仓库**（.gitignore 里排除，只保留说明文件）
    ai.local.json       把 key 放这里
```

## 怎么用

1. **跑 mock**：直接把 `mock/*.json` 的内容塞进浏览器的
   `localStorage["sharp-resume:workspace:v1"]`，刷新即可看到对应版本的数据被迁移成 v4。
2. **配密钥**：打开 `secrets/README.md` 看格式，新建 `secrets/ai.local.json`。
   它只在开发构建里被读取；打包时会拦下（`scripts/package.mjs`）。

## 为什么这样分

| 目录 | 进仓库 | 用途 |
|---|---|---|
| `ai.json` | ✅ | 团队共享的测试环境（baseUrl + model），谁都能用，没有敏感信息 |
| `mock/` | ✅ | 迁移与排版回归的固定输入 |
| `secrets/` | ❌ | 只有自己的 key，绝不能出现在 git 历史里 |

`mock/` 里的数据全部是编的（林夏/陈默/周识），不含真实简历内容。
