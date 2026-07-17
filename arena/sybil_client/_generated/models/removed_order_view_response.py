from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.reserved_position_release_response import ReservedPositionReleaseResponse





T = TypeVar("T", bound="RemovedOrderViewResponse")



@_attrs_define
class RemovedOrderViewResponse:
    """ 
        Attributes:
            account_id (int):
            exit_reason (str):
            has_been_matched (bool):
            order_id (int):
            phase (str):
            reserved_balance_released_nanos (str): Released reserved cash. Integer nanodollars; 1_000_000_000 = $1.
            active_markets (list[int] | Unset):
            rejection_reason (None | str | Unset):
            reserved_positions_released (list[ReservedPositionReleaseResponse] | Unset):
     """

    account_id: int
    exit_reason: str
    has_been_matched: bool
    order_id: int
    phase: str
    reserved_balance_released_nanos: str
    active_markets: list[int] | Unset = UNSET
    rejection_reason: None | str | Unset = UNSET
    reserved_positions_released: list[ReservedPositionReleaseResponse] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.reserved_position_release_response import ReservedPositionReleaseResponse
        account_id = self.account_id

        exit_reason = self.exit_reason

        has_been_matched = self.has_been_matched

        order_id = self.order_id

        phase = self.phase

        reserved_balance_released_nanos = self.reserved_balance_released_nanos

        active_markets: list[int] | Unset = UNSET
        if not isinstance(self.active_markets, Unset):
            active_markets = self.active_markets



        rejection_reason: None | str | Unset
        if isinstance(self.rejection_reason, Unset):
            rejection_reason = UNSET
        else:
            rejection_reason = self.rejection_reason

        reserved_positions_released: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.reserved_positions_released, Unset):
            reserved_positions_released = []
            for reserved_positions_released_item_data in self.reserved_positions_released:
                reserved_positions_released_item = reserved_positions_released_item_data.to_dict()
                reserved_positions_released.append(reserved_positions_released_item)




        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "exit_reason": exit_reason,
            "has_been_matched": has_been_matched,
            "order_id": order_id,
            "phase": phase,
            "reserved_balance_released_nanos": reserved_balance_released_nanos,
        })
        if active_markets is not UNSET:
            field_dict["active_markets"] = active_markets
        if rejection_reason is not UNSET:
            field_dict["rejection_reason"] = rejection_reason
        if reserved_positions_released is not UNSET:
            field_dict["reserved_positions_released"] = reserved_positions_released

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.reserved_position_release_response import ReservedPositionReleaseResponse
        d = dict(src_dict)
        account_id = d.pop("account_id")

        exit_reason = d.pop("exit_reason")

        has_been_matched = d.pop("has_been_matched")

        order_id = d.pop("order_id")

        phase = d.pop("phase")

        reserved_balance_released_nanos = d.pop("reserved_balance_released_nanos")

        active_markets = cast(list[int], d.pop("active_markets", UNSET))


        def _parse_rejection_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        rejection_reason = _parse_rejection_reason(d.pop("rejection_reason", UNSET))


        _reserved_positions_released = d.pop("reserved_positions_released", UNSET)
        reserved_positions_released: list[ReservedPositionReleaseResponse] | Unset = UNSET
        if _reserved_positions_released is not UNSET:
            reserved_positions_released = []
            for reserved_positions_released_item_data in _reserved_positions_released:
                reserved_positions_released_item = ReservedPositionReleaseResponse.from_dict(reserved_positions_released_item_data)



                reserved_positions_released.append(reserved_positions_released_item)


        removed_order_view_response = cls(
            account_id=account_id,
            exit_reason=exit_reason,
            has_been_matched=has_been_matched,
            order_id=order_id,
            phase=phase,
            reserved_balance_released_nanos=reserved_balance_released_nanos,
            active_markets=active_markets,
            rejection_reason=rejection_reason,
            reserved_positions_released=reserved_positions_released,
        )


        removed_order_view_response.additional_properties = d
        return removed_order_view_response

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
