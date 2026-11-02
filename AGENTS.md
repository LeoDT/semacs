## 开发规则

- 新会话先读 `docs/README.md` 和 `docs/plan.md`，按引用读取相关契约；文档维护、跨会话待办与归档遵循 `docs/README.md`，不默认通读历史记录。

- reviewer 默认每阶段至多调用一次：阶段实现基本完成、自检通过，且涉及实质性重构或跨模块行为时再集中审查；小改动、常规结果核对及审查后修复由主代理自行验证。提前或追加审查须先说明理由并征得用户同意。

- 多个tool调用时使用codemode
- 需要web搜索或者获取web页面内容时使用codemode搜索对应tool
- codemode不需要console.log你需要的输出，仔细阅读codemode说明
- `~/ghq/github.com/longbridge/gpui-kit`, `~/ghq/github.com/zed-industries/zed` 这里是gpui-kit和zed的源码，只在需要时阅读
- 如果需要在用户真实环境做测试或验证，先征求用户同意
