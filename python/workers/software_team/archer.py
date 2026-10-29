"""archer 示例 worker（迁移自 legacy ``software_team.archer_proc``）。"""

from __future__ import annotations

from typing import Any

from zgraph_worker import WorkerContext

from .._inputs import unwrap_port

DEFAULT_DESIGN_PATH: str = "doc/architectural_design.md"
DEFAULT_ORG_PATH: str = "doc/organizational_structure.md"


def run(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """产出架构设计与组织架构文档内容（legacy ``archer_proc`` 的两个产物）。

    输入：
        ``design_path``（string，可选，缺省 ``"doc/architectural_design.md"``）：架构文档路径。
        ``org_path``（string，可选，缺省 ``"doc/organizational_structure.md"``）：组织架构
        文档路径。两个端口都兼容上游整包 output dict 形态（取其同名字段）。
    输出：
        ``{"documents": {<design_path>: ..., <org_path>: ...}, "document_paths": [...]}``，
        内容与 legacy stub 一致。
    幂等性：
        纯函数，相同 ``inputs`` 输出相同，可安全重试。
    外部副作用：
        无（legacy 的 ``base_dir/doc/`` 文件写入已移除；仅 ``ctx.log`` 一条可丢日志）。
    """
    design_path: str = str(
        unwrap_port(inputs, "design_path", "design_path") or DEFAULT_DESIGN_PATH
    )
    org_path: str = str(unwrap_port(inputs, "org_path", "org_path") or DEFAULT_ORG_PATH)
    documents: dict[str, str] = {
        design_path: "# architectural_design (stub)\n",
        org_path: "# organizational_structure (stub)\n",
    }
    ctx.log(f"archer produced {design_path}, {org_path}")
    return {"documents": documents, "document_paths": list(documents)}
