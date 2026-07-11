from enum import Enum

class SystemEventResponseType8Type(str, Enum):
    ORDER_CANCELLED = "order_cancelled"

    def __str__(self) -> str:
        return str(self.value)
