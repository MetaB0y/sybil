from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.bridge_deposit_event_response import BridgeDepositEventResponse
  from ..models.bridge_withdrawal_response import BridgeWithdrawalResponse





T = TypeVar("T", bound="BridgeBlockResponse")



@_attrs_define
class BridgeBlockResponse:
    """ 
        Attributes:
            deposit_count (int):
            deposit_root_hex (str):
            consumed_deposits (list[BridgeDepositEventResponse] | Unset):
            withdrawal_leaves (list[BridgeWithdrawalResponse] | Unset):
     """

    deposit_count: int
    deposit_root_hex: str
    consumed_deposits: list[BridgeDepositEventResponse] | Unset = UNSET
    withdrawal_leaves: list[BridgeWithdrawalResponse] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.bridge_deposit_event_response import BridgeDepositEventResponse
        from ..models.bridge_withdrawal_response import BridgeWithdrawalResponse
        deposit_count = self.deposit_count

        deposit_root_hex = self.deposit_root_hex

        consumed_deposits: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.consumed_deposits, Unset):
            consumed_deposits = []
            for consumed_deposits_item_data in self.consumed_deposits:
                consumed_deposits_item = consumed_deposits_item_data.to_dict()
                consumed_deposits.append(consumed_deposits_item)



        withdrawal_leaves: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.withdrawal_leaves, Unset):
            withdrawal_leaves = []
            for withdrawal_leaves_item_data in self.withdrawal_leaves:
                withdrawal_leaves_item = withdrawal_leaves_item_data.to_dict()
                withdrawal_leaves.append(withdrawal_leaves_item)




        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "deposit_count": deposit_count,
            "deposit_root_hex": deposit_root_hex,
        })
        if consumed_deposits is not UNSET:
            field_dict["consumed_deposits"] = consumed_deposits
        if withdrawal_leaves is not UNSET:
            field_dict["withdrawal_leaves"] = withdrawal_leaves

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.bridge_deposit_event_response import BridgeDepositEventResponse
        from ..models.bridge_withdrawal_response import BridgeWithdrawalResponse
        d = dict(src_dict)
        deposit_count = d.pop("deposit_count")

        deposit_root_hex = d.pop("deposit_root_hex")

        _consumed_deposits = d.pop("consumed_deposits", UNSET)
        consumed_deposits: list[BridgeDepositEventResponse] | Unset = UNSET
        if _consumed_deposits is not UNSET:
            consumed_deposits = []
            for consumed_deposits_item_data in _consumed_deposits:
                consumed_deposits_item = BridgeDepositEventResponse.from_dict(consumed_deposits_item_data)



                consumed_deposits.append(consumed_deposits_item)


        _withdrawal_leaves = d.pop("withdrawal_leaves", UNSET)
        withdrawal_leaves: list[BridgeWithdrawalResponse] | Unset = UNSET
        if _withdrawal_leaves is not UNSET:
            withdrawal_leaves = []
            for withdrawal_leaves_item_data in _withdrawal_leaves:
                withdrawal_leaves_item = BridgeWithdrawalResponse.from_dict(withdrawal_leaves_item_data)



                withdrawal_leaves.append(withdrawal_leaves_item)


        bridge_block_response = cls(
            deposit_count=deposit_count,
            deposit_root_hex=deposit_root_hex,
            consumed_deposits=consumed_deposits,
            withdrawal_leaves=withdrawal_leaves,
        )


        bridge_block_response.additional_properties = d
        return bridge_block_response

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
