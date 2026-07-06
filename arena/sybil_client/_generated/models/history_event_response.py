from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="HistoryEventResponse")



@_attrs_define
class HistoryEventResponse:
    """ One entry in the per-account history feed (`GET /v1/accounts/{id}/events`).

        Attributes:
            block_height (int):
            category (str):
            id (str):
            timestamp_ms (int):
            type_ (str):
            amount_nanos (int | None | Unset): Event cash amount. Integer nanodollars; 1_000_000_000 = $1.
            available_nanos (int | None | Unset): Rejected-order available amount. Integer nanodollars; 1_000_000_000 = $1.
            market_id (int | None | Unset):
            order_id (int | None | Unset):
            outcome (None | str | Unset):
            payout_outcome (None | str | Unset):
            price_nanos (int | None | Unset): Event price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            qty (int | None | Unset): Event quantity. Integer share-units; 1000 units = 1 share.
            realized_pnl_nanos (int | None | Unset): Event realized PnL. Integer nanodollars; 1_000_000_000 = $1.
            reason (None | str | Unset): Rejected only: reason code (`insufficient_balance` | `insufficient_position`
                | `complete_set` | …).
            required_nanos (int | None | Unset): Rejected-order required amount. Integer nanodollars; 1_000_000_000 = $1.
            side (None | str | Unset):
     """

    block_height: int
    category: str
    id: str
    timestamp_ms: int
    type_: str
    amount_nanos: int | None | Unset = UNSET
    available_nanos: int | None | Unset = UNSET
    market_id: int | None | Unset = UNSET
    order_id: int | None | Unset = UNSET
    outcome: None | str | Unset = UNSET
    payout_outcome: None | str | Unset = UNSET
    price_nanos: int | None | Unset = UNSET
    qty: int | None | Unset = UNSET
    realized_pnl_nanos: int | None | Unset = UNSET
    reason: None | str | Unset = UNSET
    required_nanos: int | None | Unset = UNSET
    side: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        block_height = self.block_height

        category = self.category

        id = self.id

        timestamp_ms = self.timestamp_ms

        type_ = self.type_

        amount_nanos: int | None | Unset
        if isinstance(self.amount_nanos, Unset):
            amount_nanos = UNSET
        else:
            amount_nanos = self.amount_nanos

        available_nanos: int | None | Unset
        if isinstance(self.available_nanos, Unset):
            available_nanos = UNSET
        else:
            available_nanos = self.available_nanos

        market_id: int | None | Unset
        if isinstance(self.market_id, Unset):
            market_id = UNSET
        else:
            market_id = self.market_id

        order_id: int | None | Unset
        if isinstance(self.order_id, Unset):
            order_id = UNSET
        else:
            order_id = self.order_id

        outcome: None | str | Unset
        if isinstance(self.outcome, Unset):
            outcome = UNSET
        else:
            outcome = self.outcome

        payout_outcome: None | str | Unset
        if isinstance(self.payout_outcome, Unset):
            payout_outcome = UNSET
        else:
            payout_outcome = self.payout_outcome

        price_nanos: int | None | Unset
        if isinstance(self.price_nanos, Unset):
            price_nanos = UNSET
        else:
            price_nanos = self.price_nanos

        qty: int | None | Unset
        if isinstance(self.qty, Unset):
            qty = UNSET
        else:
            qty = self.qty

        realized_pnl_nanos: int | None | Unset
        if isinstance(self.realized_pnl_nanos, Unset):
            realized_pnl_nanos = UNSET
        else:
            realized_pnl_nanos = self.realized_pnl_nanos

        reason: None | str | Unset
        if isinstance(self.reason, Unset):
            reason = UNSET
        else:
            reason = self.reason

        required_nanos: int | None | Unset
        if isinstance(self.required_nanos, Unset):
            required_nanos = UNSET
        else:
            required_nanos = self.required_nanos

        side: None | str | Unset
        if isinstance(self.side, Unset):
            side = UNSET
        else:
            side = self.side


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "block_height": block_height,
            "category": category,
            "id": id,
            "timestamp_ms": timestamp_ms,
            "type": type_,
        })
        if amount_nanos is not UNSET:
            field_dict["amount_nanos"] = amount_nanos
        if available_nanos is not UNSET:
            field_dict["available_nanos"] = available_nanos
        if market_id is not UNSET:
            field_dict["market_id"] = market_id
        if order_id is not UNSET:
            field_dict["order_id"] = order_id
        if outcome is not UNSET:
            field_dict["outcome"] = outcome
        if payout_outcome is not UNSET:
            field_dict["payout_outcome"] = payout_outcome
        if price_nanos is not UNSET:
            field_dict["price_nanos"] = price_nanos
        if qty is not UNSET:
            field_dict["qty"] = qty
        if realized_pnl_nanos is not UNSET:
            field_dict["realized_pnl_nanos"] = realized_pnl_nanos
        if reason is not UNSET:
            field_dict["reason"] = reason
        if required_nanos is not UNSET:
            field_dict["required_nanos"] = required_nanos
        if side is not UNSET:
            field_dict["side"] = side

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        block_height = d.pop("block_height")

        category = d.pop("category")

        id = d.pop("id")

        timestamp_ms = d.pop("timestamp_ms")

        type_ = d.pop("type")

        def _parse_amount_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        amount_nanos = _parse_amount_nanos(d.pop("amount_nanos", UNSET))


        def _parse_available_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        available_nanos = _parse_available_nanos(d.pop("available_nanos", UNSET))


        def _parse_market_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        market_id = _parse_market_id(d.pop("market_id", UNSET))


        def _parse_order_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        order_id = _parse_order_id(d.pop("order_id", UNSET))


        def _parse_outcome(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        outcome = _parse_outcome(d.pop("outcome", UNSET))


        def _parse_payout_outcome(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        payout_outcome = _parse_payout_outcome(d.pop("payout_outcome", UNSET))


        def _parse_price_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        price_nanos = _parse_price_nanos(d.pop("price_nanos", UNSET))


        def _parse_qty(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        qty = _parse_qty(d.pop("qty", UNSET))


        def _parse_realized_pnl_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        realized_pnl_nanos = _parse_realized_pnl_nanos(d.pop("realized_pnl_nanos", UNSET))


        def _parse_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        reason = _parse_reason(d.pop("reason", UNSET))


        def _parse_required_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        required_nanos = _parse_required_nanos(d.pop("required_nanos", UNSET))


        def _parse_side(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        side = _parse_side(d.pop("side", UNSET))


        history_event_response = cls(
            block_height=block_height,
            category=category,
            id=id,
            timestamp_ms=timestamp_ms,
            type_=type_,
            amount_nanos=amount_nanos,
            available_nanos=available_nanos,
            market_id=market_id,
            order_id=order_id,
            outcome=outcome,
            payout_outcome=payout_outcome,
            price_nanos=price_nanos,
            qty=qty,
            realized_pnl_nanos=realized_pnl_nanos,
            reason=reason,
            required_nanos=required_nanos,
            side=side,
        )


        history_event_response.additional_properties = d
        return history_event_response

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
