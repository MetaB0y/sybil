from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.position_response import PositionResponse





T = TypeVar("T", bound="AccountResponse")



@_attrs_define
class AccountResponse:
    """ 
        Attributes:
            account_id (int):
            balance_nanos (int):
            positions (list[PositionResponse] | Unset):
     """

    account_id: int
    balance_nanos: int
    positions: list[PositionResponse] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.position_response import PositionResponse
        account_id = self.account_id

        balance_nanos = self.balance_nanos

        positions: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.positions, Unset):
            positions = []
            for positions_item_data in self.positions:
                positions_item = positions_item_data.to_dict()
                positions.append(positions_item)




        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "balance_nanos": balance_nanos,
        })
        if positions is not UNSET:
            field_dict["positions"] = positions

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.position_response import PositionResponse
        d = dict(src_dict)
        account_id = d.pop("account_id")

        balance_nanos = d.pop("balance_nanos")

        _positions = d.pop("positions", UNSET)
        positions: list[PositionResponse] | Unset = UNSET
        if _positions is not UNSET:
            positions = []
            for positions_item_data in _positions:
                positions_item = PositionResponse.from_dict(positions_item_data)



                positions.append(positions_item)


        account_response = cls(
            account_id=account_id,
            balance_nanos=balance_nanos,
            positions=positions,
        )


        account_response.additional_properties = d
        return account_response

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
