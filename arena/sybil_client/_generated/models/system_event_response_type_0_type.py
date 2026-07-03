from enum import Enum

class SystemEventResponseType0Type(str, Enum):
    CREATE_ACCOUNT = "create_account"

    def __str__(self) -> str:
        return str(self.value)
