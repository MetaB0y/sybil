from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="ActorMarketReceipt")



@_attrs_define
class ActorMarketReceipt:
    """ 
        Attributes:
            accepted_order_ids (list[int]):
            market_id (int):
            rejection (None | str | Unset):
            skip_reason (None | str | Unset):
     """

    accepted_order_ids: list[int]
    market_id: int
    rejection: None | str | Unset = UNSET
    skip_reason: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        accepted_order_ids = self.accepted_order_ids



        market_id = self.market_id

        rejection: None | str | Unset
        if isinstance(self.rejection, Unset):
            rejection = UNSET
        else:
            rejection = self.rejection

        skip_reason: None | str | Unset
        if isinstance(self.skip_reason, Unset):
            skip_reason = UNSET
        else:
            skip_reason = self.skip_reason


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "accepted_order_ids": accepted_order_ids,
            "market_id": market_id,
        })
        if rejection is not UNSET:
            field_dict["rejection"] = rejection
        if skip_reason is not UNSET:
            field_dict["skip_reason"] = skip_reason

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        accepted_order_ids = cast(list[int], d.pop("accepted_order_ids"))


        market_id = d.pop("market_id")

        def _parse_rejection(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        rejection = _parse_rejection(d.pop("rejection", UNSET))


        def _parse_skip_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        skip_reason = _parse_skip_reason(d.pop("skip_reason", UNSET))


        actor_market_receipt = cls(
            accepted_order_ids=accepted_order_ids,
            market_id=market_id,
            rejection=rejection,
            skip_reason=skip_reason,
        )


        actor_market_receipt.additional_properties = d
        return actor_market_receipt

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
