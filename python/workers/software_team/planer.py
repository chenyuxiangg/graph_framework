"""planer 示例 worker（迁移自 legacy ``software_team.planer_proc``）。"""

from __future__ import annotations

from typing import Any

from zgraph_worker import WorkerContext

from .._inputs import unwrap_port

DEFAULT_SPECIFICATION_PATH: str = "doc/specification.md"
DEFAULT_SPECIFICATION_TITLE: str = "specification"


def run(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """产出 specification 文档内容（legacy 里由 ``planer_proc`` 写 ``doc/specification.md``）。

    输入：
        ``specification_path``（string，可选，缺省 ``"doc/specification.md"``）：产物路径。
        ``specification_title``（string，可选，缺省 ``"specification"``）：文档标题。
        两个端口都兼容上游整包 output dict 形态（取其同名字段）。
    输出：
        ``{"documents": {<path>: <content>}, "document_paths": [<path>]}``，内容与 legacy
        stub 一致（``# <title> (stub)\\n``）。
    幂等性：
        纯函数，相同 ``inputs`` 输出相同，可安全重试。
    外部副作用：
        无（legacy 的 ``base_dir/doc/`` 文件写入已移除；仅 ``ctx.log`` 一条可丢日志）。
    """
    path: str = str(
        unwrap_port(inputs, "specification_path", "specification_path")
        or DEFAULT_SPECIFICATION_PATH
    )
    title: str = str(
        unwrap_port(inputs, "specification_title", "specification_title")
        or DEFAULT_SPECIFICATION_TITLE
    )
    content: str = f"# {title} (stub)\n"
    ctx.log(f"planer produced {path}")
    return {"documents": {path: content}, "document_paths": [path]}
