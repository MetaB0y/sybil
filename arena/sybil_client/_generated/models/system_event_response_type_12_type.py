from enum import Enum

class SystemEventResponseType12Type(str, Enum):
    CLIENT_ACTION_AUTHORIZED = "client_action_authorized"

    def __str__(self) -> str:
        return str(self.value)
