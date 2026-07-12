from enum import Enum

class AuthScheme(str, Enum):
    RAW_P256 = "raw_p256"
    WEBAUTHN = "webauthn"

    def __str__(self) -> str:
        return str(self.value)
