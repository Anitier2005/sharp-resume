#!/usr/bin/env node
/**
 * 打包：把 release 产物放进一个「干净的」交付目录。
 *
 * 规则（目录任何时候都只该有它该有的东西）：
 *   1. 目标目录先整体清空，再只放一个 exe —— 不留上次的残留（.new.exe 之类）
 *   2. 目标目录里若有运行数据 SharpResumeData，默认拒绝并提示，
 *      避免把用户的工作区连同目录一起删掉；确认要删再加 --force
 *   3. 复制后自检：目录里必须恰好一个文件，且大小与源一致
 *
 * 用法：
 *   node scripts/package.mjs                  # 默认输出到 ../release-clean
 *   node scripts/package.mjs ../release-dist  # 输出到别处
 *   node scripts/package.mjs ../release-clean --force   # 连运行数据一起清
 */
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import process from 'node:process'

const args = process.argv.slice(2)
const force = args.includes('--force')
const target = path.resolve(args.find((a) => !a.startsWith('--')) ?? '../release-clean')

const EXE_NAME = 'sharp-resume.exe'
const SRC = path.resolve('src-tauri/target/release', EXE_NAME)
const DIST_HTML = path.resolve('dist/index.html')

function fail(message) {
  console.error(`[FAIL] ${message}`)
  process.exit(1)
}

function info(message) {
  console.log(`[info] ${message}`)
}

/** 目录里非空则列出内容，供清理前提示 */
function listDir(dir) {
  try {
    return fs.readdirSync(dir)
  } catch {
    return []
  }
}

if (!fs.existsSync(SRC)) {
  fail(`找不到构建产物 ${SRC}\n       先跑一次 release 构建（见 README「Windows 打包」）`)
}

// 前端产物比 exe 新 → exe 里内嵌的是旧前端，容易以为是样式没生效
if (fs.existsSync(DIST_HTML)) {
  const exeTime = fs.statSync(SRC).mtimeMs
  const distTime = fs.statSync(DIST_HTML).mtimeMs
  if (distTime > exeTime + 1000) {
    fail(
      'dist/ 比 exe 新，说明改完前端还没重新编译 Rust。\n' +
        '       先重新构建，否则打出来的 exe 里是旧界面。',
    )
  }
}

// —— 密钥保护：本机密钥文件绝不能被打进发行版 ——
// 它在 dev 下是默认配置，若打包时还在，会被 vite 静态引入进 bundle。
const LOCAL_KEY = path.resolve('config/ai.local.json')
if (fs.existsSync(LOCAL_KEY)) {
  fail(
    '发现 config/ai.local.json（本机密钥）。\n' +
      '       它会被编进前端 bundle，发行版等于把 key 送出去。\n' +
      '       打包前先把它移走（例如 mv config/ai.local.json ~/ai.local.json.bak）。',
  )
}

// —— 数据保护：交付目录里的 SharpResumeData 是真实运行数据 ——
const dataDir = path.join(target, 'SharpResumeData')
if (fs.existsSync(dataDir) && !force) {
  const files = listDir(dataDir)
  fail(
    `目标目录里有运行数据：${dataDir}\n` +
      `       （${files.length} 个文件：${files.slice(0, 3).join('、')}${files.length > 3 ? '…' : ''}）\n` +
      '       打包会清空整个目录，请先把它移走；确认不要了再加 --force。',
  )
}

// —— 清空目标目录 ——
const existed = fs.existsSync(target)
const leftovers = existed ? listDir(target) : []
if (existed) {
  try {
    fs.rmSync(target, { recursive: true, force: true })
  } catch (error) {
    if (error.code === 'EBUSY' || error.code === 'EPERM') {
      fail(
        `删不掉 ${target}：可能有本程序正在运行（exe 被占用）。\n` +
          '       关掉程序后重试。',
      )
    }
    throw error
  }
  if (leftovers.length) {
    info(`已清掉上次的 ${leftovers.length} 项：${leftovers.join('、')}`)
  }
}
fs.mkdirSync(target, { recursive: true })

// —— 复制 ——
fs.copyFileSync(SRC, path.join(target, EXE_NAME))

// —— 自检：目录里只能有一个文件，且与源一致 ——
const finalFiles = fs.readdirSync(target)
if (finalFiles.length !== 1 || finalFiles[0] !== EXE_NAME) {
  fail(`自检失败：目录内容异常 → ${finalFiles.join('、') || '(空)'}`)
}

const srcSize = fs.statSync(SRC).size
const outSize = fs.statSync(path.join(target, EXE_NAME)).size
if (srcSize !== outSize) {
  fail(`自检失败：大小不一致 源=${srcSize} 产物=${outSize}`)
}

const sha = createHash('sha256')
  .update(fs.readFileSync(path.join(target, EXE_NAME)))
  .digest('hex')
  .slice(0, 16)

console.log('')
console.log(`[ok] ${target}`)
console.log(`     ${EXE_NAME}  ${(outSize / 1024 / 1024).toFixed(2)} MB  sha256:${sha}…`)
console.log(`     目录内文件数：${finalFiles.length}（干净）`)
