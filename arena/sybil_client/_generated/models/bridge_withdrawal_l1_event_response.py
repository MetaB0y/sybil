from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.bridge_withdrawal_response import BridgeWithdrawalResponse





T = TypeVar("T", bound="BridgeWithdrawalL1EventResponse")



@_attrs_define
class BridgeWithdrawalL1EventResponse:
    """ 
        Attributes:
            active_withdrawal_found (bool): False when the terminal withdrawal was already pruned; the observation
                is still accepted as an idempotent no-op.
            withdrawal (BridgeWithdrawalResponse | None | Unset):
     """

    active_withdrawal_found: bool
    withdrawal: BridgeWithdrawalResponse | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.bridge_withdrawal_response import BridgeWithdrawalResponse
        active_withdrawal_found = self.active_withdrawal_found

        withdrawal: dict[str, Any] | None | Unset
        if isinstance(self.withdrawal, Unset):
            withdrawal = UNSET
        elif isinstance(self.withdrawal, BridgeWithdrawalResponse):
            withdrawal = self.withdrawal.to_dict()
        else:
            withdrawal = self.withdrawal


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "active_withdrawal_found": active_withdrawal_found,
        })
        if withdrawal is not UNSET:
            field_dict["withdrawal"] = withdrawal

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.bridge_withdrawal_response import BridgeWithdrawalResponse
        d = dict(src_dict)
        active_withdrawal_found = d.pop("active_withdrawal_found")

        def _parse_withdrawal(data: object) -> BridgeWithdrawalResponse | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                withdrawal_type_1 = BridgeWithdrawalResponse.from_dict(data)



                return withdrawal_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(BridgeWithdrawalResponse | None | Unset, data)

        withdrawal = _parse_withdrawal(d.pop("withdrawal", UNSET))


        bridge_withdrawal_l1_event_response = cls(
            active_withdrawal_found=active_withdrawal_found,
            withdrawal=withdrawal,
        )


        bridge_withdrawal_l1_event_response.additional_properties = d
        return bridge_withdrawal_l1_event_response

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
