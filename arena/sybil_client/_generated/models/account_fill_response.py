from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.position_delta_response import PositionDeltaResponse





T = TypeVar("T", bound="AccountFillResponse")



@_attrs_define
class AccountFillResponse:
    """ 
        Attributes:
            block_height (int):
            cursor (str): Stable cursor for forward pagination (`GET .../fills?after=<cursor>`).
                Opaque to clients; current encoding is `<block_height>.<order_id>`.
            fill_price_nanos (str): Fill price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            fill_qty (int): Fill quantity. Integer share-units; 1000 units = 1 share.
            order_id (int):
            position_deltas (list[PositionDeltaResponse]):
            timestamp_ms (int):
     """

    block_height: int
    cursor: str
    fill_price_nanos: str
    fill_qty: int
    order_id: int
    position_deltas: list[PositionDeltaResponse]
    timestamp_ms: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.position_delta_response import PositionDeltaResponse
        block_height = self.block_height

        cursor = self.cursor

        fill_price_nanos = self.fill_price_nanos

        fill_qty = self.fill_qty

        order_id = self.order_id

        position_deltas = []
        for position_deltas_item_data in self.position_deltas:
            position_deltas_item = position_deltas_item_data.to_dict()
            position_deltas.append(position_deltas_item)



        timestamp_ms = self.timestamp_ms


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "block_height": block_height,
            "cursor": cursor,
            "fill_price_nanos": fill_price_nanos,
            "fill_qty": fill_qty,
            "order_id": order_id,
            "position_deltas": position_deltas,
            "timestamp_ms": timestamp_ms,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.position_delta_response import PositionDeltaResponse
        d = dict(src_dict)
        block_height = d.pop("block_height")

        cursor = d.pop("cursor")

        fill_price_nanos = d.pop("fill_price_nanos")

        fill_qty = d.pop("fill_qty")

        order_id = d.pop("order_id")

        position_deltas = []
        _position_deltas = d.pop("position_deltas")
        for position_deltas_item_data in (_position_deltas):
            position_deltas_item = PositionDeltaResponse.from_dict(position_deltas_item_data)



            position_deltas.append(position_deltas_item)


        timestamp_ms = d.pop("timestamp_ms")

        account_fill_response = cls(
            block_height=block_height,
            cursor=cursor,
            fill_price_nanos=fill_price_nanos,
            fill_qty=fill_qty,
            order_id=order_id,
            position_deltas=position_deltas,
            timestamp_ms=timestamp_ms,
        )


        account_fill_response.additional_properties = d
        return account_fill_response

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
