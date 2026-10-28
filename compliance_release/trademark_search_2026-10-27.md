# 商标与生态检索记录 — mang / 芒

> 对应 `packaging/checklist.md` A2（TRADEMARK_RULES §4 五步检索）与 A3（denylist 复跑）。
> 检索日期：2026-10-27
> 检索执行：Lead（脚本化 + 公开数据源）；**CNIPA / WIPO 需人工补**（见 §4）。

---

## 1. 定名决议

| 项 | 值 |
|---|---|
| 对外显示名 | **mang** |
| 中文名 | **芒** |
| 决议日期 | 2026-10-27 |
| 决议人 | 项目所有者 |
| 可执行名 / 包名 | `mdcore`（不变，TRADEMARK_RULES §1 允许域） |
| **用途性质** | **非商用显示名**——所有者明确：不注册域名、不承诺商用 |
| 三件套口径 | 域名项**豁免**；仓库组织名维持 `mmqz` |

---

## 2. 已完成的机器化检索

### 2.1 开发者生态（crates.io / npm）

| 源 | 结果 | 说明 |
|---|---|---|
| **crates.io** | ✅ **可用** | `GET https://crates.io/api/v1/crates/mang` → 404 |
| **npm** | ❌ **已占用** | `GET https://registry.npmjs.org/mang` → 200 |

**npm 已占用的影响**：web 前端若要发布 npm 包，必须走 scope（如 `@mang/editor`），
不能占用裸名 `mang`。**不影响产品定名**（产品名 ≠ npm 包名）。

### 2.2 GitHub 账号 / 组织

| 项 | 结果 |
|---|---|
| `github.com/mang`（User） | ❌ 已占用 |
| `github.com/mang`（Org） | ✅ 组织名可用（User 同名不冲突于 Org 创建？需实际尝试） |

**建议**：仓库组织名保持 `mmqz`（三件套一致性按 A1 复核），或新建 Org 需重新评估。

### 2.3 域名可用性（HTTP 探测，非 whois）

| 域名 | 探测结果 | 判定 |
|---|---|---|
| `mang.dev` | LIVE(200) | ❌ 已注册 |
| `mang.app` | RESPOND(301) | ❌ 已注册 |
| `getmang.com` | LIVE(200) | ❌ 已注册 |
| `mang.com` | 无响应 | ⚠️ **需 whois 确认**（4 字母 .com 极大概率已注册） |
| `mang.cn` | 无响应 | ⚠️ 需 whois |
| `mang.io` | 无响应 | ⚠️ 需 whois |
| `mang.tech` | 无响应 | ⚠️ 需 whois |
| `mangdoc.com` | 无响应 | ⚠️ 需 whois |

**HTTP 探测不等于未注册**——已注册但无 A 记录/未建站会显示无响应。
**A2 要求以 whois 为准，本表仅为初筛。**

### 2.4 A3 · denylist 复跑（参考对象近似判定）

`compliance/denylist/sources.txt` 全词条：

```
typora Typora TYPORA  muya Muya MUYA  d1_ d2_  Delta  zed Zed ZED
gpui GPUI  sum_tree SumTree  language_tree  obsidian Obsidian OBSIDIAN
```

| 判定 | 结果 |
|---|---|
| denylist 是否含 `mang` | ❌ 不含 |
| `mang` vs typora / muya / delta / zed / gpui / obsidian | ✅ 全部不近似 |

**结论**：A3 的「新名与四参考对象无近似混淆」**通过**。
（`scan.sh --all` 的实际执行需 bash 环境，本轮在 Windows 上以等价词集核验；
发布前仍应在 Linux/CI 上跑一次原脚本。）

---

## 3. 已知风险（决议时已接受）

| # | 风险 | 说明 |
|---|---|---|
| R-1 | **ma- 词族联想** | 英语读者易补全为 mango / manga / mangle；`mangle` 有「撕裂/损毁」负面义 |
| R-2 | **Mango 系产品在同类目** | MangoApps（企业协作）、mangopractice（文档管理）——搜索会混淆 |
| R-3 | **中文「忙」歧义** | 拼音 mang 对应「忙」（负面）/「芒」（中性） |
| R-4 | **npm 裸名已占** | web 包需 scope |
| R-5 | **4 字母域名基本售罄** | .dev/.app/getmang 已占，.com 极可能已占 |

**决议立场**：接受 R-1~R-5，以 **mang / 芒** 定名。
理由：品牌区隔靠产品形态与视觉识别，不依赖域名精确匹配；
若后续商标检索（§4）在 9/42 类遭遇近似驳回，回退方案见 §5。

---

## 4. 待人工完成的检索（A2 已降级为风险登记，**不阻塞 A4**）

| # | 检索项 | 状态 | 负责 |
|---|---|---|---|
| A2-1 | 中国商标网（CNIPA）第 9 类 / 第 42 类同名与近似 | ⬜ **未完成** | 人工 |
| A2-2 | WIPO Global Brand Database 国际注册 | ⬜ **未完成** | 人工 |
| A2-3 | common-law 使用证据（应用商店 / Product Hunt / 搜索引擎） | ⬜ **未完成** | 人工 |
| A2-4 | 域名 whois 精确判定（§2.3 标 ⚠️ 项） | ⬜ **未完成** | 人工 |
| A2-5 | 账号名可用性最终确认（GitHub Org / 应用商店开发者名） | ⬜ **未完成** | 人工 |

**处置（2026-10-27 所有者决议后）**：A2 降级为**风险登记**，不再阻塞 A4。

- A4 已于 2026-10-27 执行（`{{APP_NAME}}` → `mang`，5 个模板/脚本 + 门禁断言）；
- 上表 A2-1~A2-5 **仍为未完成**，保留在档；
- **触发条件**：一旦本软件转为**商用或公开发行**，必须完成 A2-1~A2-5 并重评定名。

> ⚠️ **另有一处流程缺口**：本记录与 `packaging/checklist.md` 均引用
> `TRADEMARK_RULES` 的 §1/§2.5/§4，但**该文件在仓库中不存在**。
> A2-1~A2-5 的具体判定标准（近似标准、类别范围、驳回处置）因此无据可依，
> 需先补 `TRADEMARK_RULES.md`（或确认其存于他处）。

---

## 5. 回退方案

若 CNIPA 9/42 类检索发现同名/近似导致不可注册：

| 方案 | 说明 |
|---|---|
| B-1 | 加后缀区隔：`mang writer` / `mangdoc` / `mang studio`（需重跑近似判定） |
| B-2 | 换形近字：中文保留「芒」，英文换臆造词（如 `mangor` 不可——更近 mango） |
| B-3 | 完全改名，重新走 A1-A3 |

**回退时本文件追加「修订记录」小节，不覆盖原检索结论。**

---

## 6. 检索方法与可复现命令

```powershell
# crates.io / npm 可用性
Invoke-WebRequest -Uri 'https://crates.io/api/v1/crates/mang' -UseBasicParsing   # 404 = 可用
Invoke-WebRequest -Uri 'https://registry.npmjs.org/mang' -UseBasicParsing       # 200 = 已占用

# GitHub 账号/组织
gh api users/mang
gh api orgs/mang

# denylist 复跑（Linux/CI）
bash compliance/scan.sh --all
```