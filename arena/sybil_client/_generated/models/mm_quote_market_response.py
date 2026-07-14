from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="MmQuoteMarketResponse")



@_attrs_define
class MmQuoteMarketResponse:
    """ 
        Attributes:
            market_id (int):
            quote_state (str):
            ask_quantity (int | None | Unset):
            bid_quantity (int | None | Unset):
            skip_reason (None | str | Unset):
            yes_ask_nanos (int | None | Unset): Economic YES ask. Integer nanodollars per share; 1_000_000_000 = $1.
            yes_bid_nanos (int | None | Unset): Economic YES bid. Integer nanodollars per share; 1_000_000_000 = $1.
     """

    market_id: int
    quote_state: str
    ask_quantity: int | None | Unset = UNSET
    bid_quantity: int | None | Unset = UNSET
    skip_reason: None | str | Unset = UNSET
    yes_ask_nanos: int | None | Unset = UNSET
    yes_bid_nanos: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        market_id = self.market_id

        quote_state = self.quote_state

        ask_quantity: int | None | Unset
        if isinstance(self.ask_quantity, Unset):
            ask_quantity = UNSET
        else:
            ask_quantity = self.ask_quantity

        bid_quantity: int | None | Unset
        if isinstance(self.bid_quantity, Unset):
            bid_quantity = UNSET
        else:
            bid_quantity = self.bid_quantity

        skip_reason: None | str | Unset
        if isinstance(self.skip_reason, Unset):
            skip_reason = UNSET
        else:
            skip_reason = self.skip_reason

        yes_ask_nanos: int | None | Unset
        if isinstance(self.yes_ask_nanos, Unset):
            yes_ask_nanos = UNSET
        else:
            yes_ask_nanos = self.yes_ask_nanos

        yes_bid_nanos: int | None | Unset
        if isinstance(self.yes_bid_nanos, Unset):
            yes_bid_nanos = UNSET
        else:
            yes_bid_nanos = self.yes_bid_nanos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "market_id": market_id,
            "quote_state": quote_state,
        })
        if ask_quantity is not UNSET:
            field_dict["ask_quantity"] = ask_quantity
        if bid_quantity is not UNSET:
            field_dict["bid_quantity"] = bid_quantity
        if skip_reason is not UNSET:
            field_dict["skip_reason"] = skip_reason
        if yes_ask_nanos is not UNSET:
            field_dict["yes_ask_nanos"] = yes_ask_nanos
        if yes_bid_nanos is not UNSET:
            field_dict["yes_bid_nanos"] = yes_bid_nanos

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        market_id = d.pop("market_id")

        quote_state = d.pop("quote_state")

        def _parse_ask_quantity(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ask_quantity = _parse_ask_quantity(d.pop("ask_quantity", UNSET))


        def _parse_bid_quantity(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        bid_quantity = _parse_bid_quantity(d.pop("bid_quantity", UNSET))


        def _parse_skip_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        skip_reason = _parse_skip_reason(d.pop("skip_reason", UNSET))


        def _parse_yes_ask_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        yes_ask_nanos = _parse_yes_ask_nanos(d.pop("yes_ask_nanos", UNSET))


        def _parse_yes_bid_nanos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        yes_bid_nanos = _parse_yes_bid_nanos(d.pop("yes_bid_nanos", UNSET))


        mm_quote_market_response = cls(
            market_id=market_id,
            quote_state=quote_state,
            ask_quantity=ask_quantity,
            bid_quantity=bid_quantity,
            skip_reason=skip_reason,
            yes_ask_nanos=yes_ask_nanos,
            yes_bid_nanos=yes_bid_nanos,
        )


        mm_quote_market_response.additional_properties = d
        return mm_quote_market_response

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
