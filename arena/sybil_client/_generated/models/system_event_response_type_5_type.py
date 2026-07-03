from enum import Enum

class SystemEventResponseType5Type(str, Enum):
    ORDER_CANCELLED = "order_cancelled"

    def __str__(self) -> str:
        return str(self.value)
