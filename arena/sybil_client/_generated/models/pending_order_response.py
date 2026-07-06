from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset






T = TypeVar("T", bound="PendingOrderResponse")



@_attrs_define
class PendingOrderResponse:
    """ 
        Attributes:
            account_id (int):
            created_at_block (int):
            expires_at_block (int):
            limit_price_nanos (int): Limit price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            market_id (int):
            order_id (int):
            remaining_quantity (int): Remaining fill quantity. Integer share-units; 1000 units = 1 share.
            side (str):
            created_at_ms (int | Unset): Wall-clock admit time, ms since epoch. `0` for orders admitted before
                this field shipped (#[serde(default)] forward compat).
            original_quantity (int | Unset): Original `max_fill` at admit time. Integer share-units; 1000 units = 1 share.
                Lets the FE render a partial-fill progress bar as
                `(original - remaining) / original`.
                `0` for orders persisted before B5/B8 (#[serde(default)] forward
                compat).
     """

    account_id: int
    created_at_block: int
    expires_at_block: int
    limit_price_nanos: int
    market_id: int
    order_id: int
    remaining_quantity: int
    side: str
    created_at_ms: int | Unset = UNSET
    original_quantity: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        created_at_block = self.created_at_block

        expires_at_block = self.expires_at_block

        limit_price_nanos = self.limit_price_nanos

        market_id = self.market_id

        order_id = self.order_id

        remaining_quantity = self.remaining_quantity

        side = self.side

        created_at_ms = self.created_at_ms

        original_quantity = self.original_quantity


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "created_at_block": created_at_block,
            "expires_at_block": expires_at_block,
            "limit_price_nanos": limit_price_nanos,
            "market_id": market_id,
            "order_id": order_id,
            "remaining_quantity": remaining_quantity,
            "side": side,
        })
        if created_at_ms is not UNSET:
            field_dict["created_at_ms"] = created_at_ms
        if original_quantity is not UNSET:
            field_dict["original_quantity"] = original_quantity

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        created_at_block = d.pop("created_at_block")

        expires_at_block = d.pop("expires_at_block")

        limit_price_nanos = d.pop("limit_price_nanos")

        market_id = d.pop("market_id")

        order_id = d.pop("order_id")

        remaining_quantity = d.pop("remaining_quantity")

        side = d.pop("side")

        created_at_ms = d.pop("created_at_ms", UNSET)

        original_quantity = d.pop("original_quantity", UNSET)

        pending_order_response = cls(
            account_id=account_id,
            created_at_block=created_at_block,
            expires_at_block=expires_at_block,
            limit_price_nanos=limit_price_nanos,
            market_id=market_id,
            order_id=order_id,
            remaining_quantity=remaining_quantity,
            side=side,
            created_at_ms=created_at_ms,
            original_quantity=original_quantity,
        )


        pending_order_response.additional_properties = d
        return pending_order_response

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
