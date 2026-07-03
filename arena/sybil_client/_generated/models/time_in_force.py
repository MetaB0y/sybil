from enum import Enum

class TimeInForce(str, Enum):
    GTC = "GTC"
    GTD = "GTD"
    IOC = "IOC"

    def __str__(self) -> str:
        return str(self.value)
