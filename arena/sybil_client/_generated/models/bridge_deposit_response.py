from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="BridgeDepositResponse")



@_attrs_define
class BridgeDepositResponse:
    """ 
        Attributes:
            account_id (int):
            balance_nanos (int):
            deposit_id (int):
            deposit_root_hex (str):
     """

    account_id: int
    balance_nanos: int
    deposit_id: int
    deposit_root_hex: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        balance_nanos = self.balance_nanos

        deposit_id = self.deposit_id

        deposit_root_hex = self.deposit_root_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "balance_nanos": balance_nanos,
            "deposit_id": deposit_id,
            "deposit_root_hex": deposit_root_hex,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        balance_nanos = d.pop("balance_nanos")

        deposit_id = d.pop("deposit_id")

        deposit_root_hex = d.pop("deposit_root_hex")

        bridge_deposit_response = cls(
            account_id=account_id,
            balance_nanos=balance_nanos,
            deposit_id=deposit_id,
            deposit_root_hex=deposit_root_hex,
        )


        bridge_deposit_response.additional_properties = d
        return bridge_deposit_response

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
