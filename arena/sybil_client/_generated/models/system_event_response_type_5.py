from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..models.system_event_response_type_5_type import SystemEventResponseType5Type
from typing import cast






T = TypeVar("T", bound="SystemEventResponseType5")



@_attrs_define
class SystemEventResponseType5:
    """ On-chain cancellation event (D1). `side` is the categorical
    `OrderDirection` ("BuyYes"/"SellYes"/"BuyNo"/"SellNo") and
    `remaining_quantity` is the unfilled portion of `max_fill` at
    cancel time, in fixed-point share-units. Forward-additive: old clients
    ignore unknown
    variants via serde's `#[serde(tag = "type")]` shape.

        Attributes:
            account_id (int):
            market_ids (list[int]):
            order_id (int):
            remaining_quantity (int):
            side (str):
            type_ (SystemEventResponseType5Type):
     """

    account_id: int
    market_ids: list[int]
    order_id: int
    remaining_quantity: int
    side: str
    type_: SystemEventResponseType5Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        market_ids = self.market_ids



        order_id = self.order_id

        remaining_quantity = self.remaining_quantity

        side = self.side

        type_ = self.type_.value


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "market_ids": market_ids,
            "order_id": order_id,
            "remaining_quantity": remaining_quantity,
            "side": side,
            "type": type_,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        market_ids = cast(list[int], d.pop("market_ids"))


        order_id = d.pop("order_id")

        remaining_quantity = d.pop("remaining_quantity")

        side = d.pop("side")

        type_ = SystemEventResponseType5Type(d.pop("type"))




        system_event_response_type_5 = cls(
            account_id=account_id,
            market_ids=market_ids,
            order_id=order_id,
            remaining_quantity=remaining_quantity,
            side=side,
            type_=type_,
        )


        system_event_response_type_5.additional_properties = d
        return system_event_response_type_5

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
