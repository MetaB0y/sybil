from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="BridgeDepositResponse")



@_attrs_define
class BridgeDepositResponse:
    """ 
        Attributes:
            deposit_id (int):
            deposit_root_hex (str):
            disposition (str): `credited` or `quarantined`.
            account_id (int | None | Unset):
            balance_nanos (None | str | Unset): Account balance after the deposit. Integer nanodollars; 1_000_000_000 = $1.
     """

    deposit_id: int
    deposit_root_hex: str
    disposition: str
    account_id: int | None | Unset = UNSET
    balance_nanos: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        deposit_id = self.deposit_id

        deposit_root_hex = self.deposit_root_hex

        disposition = self.disposition

        account_id: int | None | Unset
        if isinstance(self.account_id, Unset):
            account_id = UNSET
        else:
            account_id = self.account_id

        balance_nanos: None | str | Unset
        if isinstance(self.balance_nanos, Unset):
            balance_nanos = UNSET
        else:
            balance_nanos = self.balance_nanos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "deposit_id": deposit_id,
            "deposit_root_hex": deposit_root_hex,
            "disposition": disposition,
        })
        if account_id is not UNSET:
            field_dict["account_id"] = account_id
        if balance_nanos is not UNSET:
            field_dict["balance_nanos"] = balance_nanos

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        deposit_id = d.pop("deposit_id")

        deposit_root_hex = d.pop("deposit_root_hex")

        disposition = d.pop("disposition")

        def _parse_account_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        account_id = _parse_account_id(d.pop("account_id", UNSET))


        def _parse_balance_nanos(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        balance_nanos = _parse_balance_nanos(d.pop("balance_nanos", UNSET))


        bridge_deposit_response = cls(
            deposit_id=deposit_id,
            deposit_root_hex=deposit_root_hex,
            disposition=disposition,
            account_id=account_id,
            balance_nanos=balance_nanos,
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
