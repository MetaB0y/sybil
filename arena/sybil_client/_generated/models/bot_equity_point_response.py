from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="BotEquityPointResponse")



@_attrs_define
class BotEquityPointResponse:
    """ 
        Attributes:
            id (int):
            trader_name (str):
            balance (float | None | Unset):
            pnl (float | None | Unset):
            portfolio_value (float | None | Unset):
            timestamp (None | str | Unset):
            total_fills (int | None | Unset):
            total_orders (int | None | Unset):
     """

    id: int
    trader_name: str
    balance: float | None | Unset = UNSET
    pnl: float | None | Unset = UNSET
    portfolio_value: float | None | Unset = UNSET
    timestamp: None | str | Unset = UNSET
    total_fills: int | None | Unset = UNSET
    total_orders: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        id = self.id

        trader_name = self.trader_name

        balance: float | None | Unset
        if isinstance(self.balance, Unset):
            balance = UNSET
        else:
            balance = self.balance

        pnl: float | None | Unset
        if isinstance(self.pnl, Unset):
            pnl = UNSET
        else:
            pnl = self.pnl

        portfolio_value: float | None | Unset
        if isinstance(self.portfolio_value, Unset):
            portfolio_value = UNSET
        else:
            portfolio_value = self.portfolio_value

        timestamp: None | str | Unset
        if isinstance(self.timestamp, Unset):
            timestamp = UNSET
        else:
            timestamp = self.timestamp

        total_fills: int | None | Unset
        if isinstance(self.total_fills, Unset):
            total_fills = UNSET
        else:
            total_fills = self.total_fills

        total_orders: int | None | Unset
        if isinstance(self.total_orders, Unset):
            total_orders = UNSET
        else:
            total_orders = self.total_orders


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "id": id,
            "trader_name": trader_name,
        })
        if balance is not UNSET:
            field_dict["balance"] = balance
        if pnl is not UNSET:
            field_dict["pnl"] = pnl
        if portfolio_value is not UNSET:
            field_dict["portfolio_value"] = portfolio_value
        if timestamp is not UNSET:
            field_dict["timestamp"] = timestamp
        if total_fills is not UNSET:
            field_dict["total_fills"] = total_fills
        if total_orders is not UNSET:
            field_dict["total_orders"] = total_orders

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        trader_name = d.pop("trader_name")

        def _parse_balance(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        balance = _parse_balance(d.pop("balance", UNSET))


        def _parse_pnl(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        pnl = _parse_pnl(d.pop("pnl", UNSET))


        def _parse_portfolio_value(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        portfolio_value = _parse_portfolio_value(d.pop("portfolio_value", UNSET))


        def _parse_timestamp(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        timestamp = _parse_timestamp(d.pop("timestamp", UNSET))


        def _parse_total_fills(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        total_fills = _parse_total_fills(d.pop("total_fills", UNSET))


        def _parse_total_orders(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        total_orders = _parse_total_orders(d.pop("total_orders", UNSET))


        bot_equity_point_response = cls(
            id=id,
            trader_name=trader_name,
            balance=balance,
            pnl=pnl,
            portfolio_value=portfolio_value,
            timestamp=timestamp,
            total_fills=total_fills,
            total_orders=total_orders,
        )


        bot_equity_point_response.additional_properties = d
        return bot_equity_point_response

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
