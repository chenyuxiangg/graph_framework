"""worker 异常类型与错误码（与 ``doc/architecture-v2.md`` §8.2 对齐）。"""

from __future__ import annotations

CODE_PYTHON_EXCEPTION: str = "NODE_PYTHON_EXCEPTION"
CODE_BUSINESS_ERROR: str = "NODE_BUSINESS_ERROR"
CODE_CANCELLED: str = "NODE_CANCELLED"
CODE_IMPORT: str = "NODE_WORKER_IMPORT"
CODE_PROTOCOL: str = "NODE_WORKER_PROTOCOL"
CODE_RETURN_NOT_DICT: str = "NODE_RETURN_NOT_DICT"


class WorkerError(Exception):
    """worker 侧可预期错误基类（会转成结构化 ``error`` 帧）。"""

    code: str = CODE_PYTHON_EXCEPTION

    def __init__(self, message: str, *, details: dict[str, object] | None = None) -> None:
        super().__init__(message)
        self.message: str = message
        self.details: dict[str, object] | None = details


class BusinessError(WorkerError):
    """节点业务错误（预期内失败）。"""

    code = CODE_BUSINESS_ERROR


class WorkerCancelled(WorkerError):
    """协作取消（``ctx.check_cancelled()`` 抛出）。"""

    code = CODE_CANCELLED


class WorkerImportError(WorkerError):
    """模块/函数导入失败或不可调用。"""

    code = CODE_IMPORT


class ReturnNotDictError(WorkerError):
    """节点返回值不是 dict（协议要求 result.output 为对象）。"""

    code = CODE_RETURN_NOT_DICT
