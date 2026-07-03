from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="BotDecisionResponse")



@_attrs_define
class BotDecisionResponse:
    """ 
        Attributes:
            article_urls (Any):
            id (int):
            orders (Any):
            trader_name (str):
            analysis (None | str | Unset):
            balance (float | None | Unset):
            edge (float | None | Unset):
            fair_value (float | None | Unset):
            llm_duration_s (float | None | Unset):
            market_id (int | None | Unset):
            market_name (None | str | Unset):
            market_price (float | None | Unset):
            motivation (None | str | Unset):
            no_pos (int | None | Unset):
            timestamp (None | str | Unset):
            yes_pos (int | None | Unset):
     """

    article_urls: Any
    id: int
    orders: Any
    trader_name: str
    analysis: None | str | Unset = UNSET
    balance: float | None | Unset = UNSET
    edge: float | None | Unset = UNSET
    fair_value: float | None | Unset = UNSET
    llm_duration_s: float | None | Unset = UNSET
    market_id: int | None | Unset = UNSET
    market_name: None | str | Unset = UNSET
    market_price: float | None | Unset = UNSET
    motivation: None | str | Unset = UNSET
    no_pos: int | None | Unset = UNSET
    timestamp: None | str | Unset = UNSET
    yes_pos: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        article_urls = self.article_urls

        id = self.id

        orders = self.orders

        trader_name = self.trader_name

        analysis: None | str | Unset
        if isinstance(self.analysis, Unset):
            analysis = UNSET
        else:
            analysis = self.analysis

        balance: float | None | Unset
        if isinstance(self.balance, Unset):
            balance = UNSET
        else:
            balance = self.balance

        edge: float | None | Unset
        if isinstance(self.edge, Unset):
            edge = UNSET
        else:
            edge = self.edge

        fair_value: float | None | Unset
        if isinstance(self.fair_value, Unset):
            fair_value = UNSET
        else:
            fair_value = self.fair_value

        llm_duration_s: float | None | Unset
        if isinstance(self.llm_duration_s, Unset):
            llm_duration_s = UNSET
        else:
            llm_duration_s = self.llm_duration_s

        market_id: int | None | Unset
        if isinstance(self.market_id, Unset):
            market_id = UNSET
        else:
            market_id = self.market_id

        market_name: None | str | Unset
        if isinstance(self.market_name, Unset):
            market_name = UNSET
        else:
            market_name = self.market_name

        market_price: float | None | Unset
        if isinstance(self.market_price, Unset):
            market_price = UNSET
        else:
            market_price = self.market_price

        motivation: None | str | Unset
        if isinstance(self.motivation, Unset):
            motivation = UNSET
        else:
            motivation = self.motivation

        no_pos: int | None | Unset
        if isinstance(self.no_pos, Unset):
            no_pos = UNSET
        else:
            no_pos = self.no_pos

        timestamp: None | str | Unset
        if isinstance(self.timestamp, Unset):
            timestamp = UNSET
        else:
            timestamp = self.timestamp

        yes_pos: int | None | Unset
        if isinstance(self.yes_pos, Unset):
            yes_pos = UNSET
        else:
            yes_pos = self.yes_pos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "article_urls": article_urls,
            "id": id,
            "orders": orders,
            "trader_name": trader_name,
        })
        if analysis is not UNSET:
            field_dict["analysis"] = analysis
        if balance is not UNSET:
            field_dict["balance"] = balance
        if edge is not UNSET:
            field_dict["edge"] = edge
        if fair_value is not UNSET:
            field_dict["fair_value"] = fair_value
        if llm_duration_s is not UNSET:
            field_dict["llm_duration_s"] = llm_duration_s
        if market_id is not UNSET:
            field_dict["market_id"] = market_id
        if market_name is not UNSET:
            field_dict["market_name"] = market_name
        if market_price is not UNSET:
            field_dict["market_price"] = market_price
        if motivation is not UNSET:
            field_dict["motivation"] = motivation
        if no_pos is not UNSET:
            field_dict["no_pos"] = no_pos
        if timestamp is not UNSET:
            field_dict["timestamp"] = timestamp
        if yes_pos is not UNSET:
            field_dict["yes_pos"] = yes_pos

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        article_urls = d.pop("article_urls")

        id = d.pop("id")

        orders = d.pop("orders")

        trader_name = d.pop("trader_name")

        def _parse_analysis(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        analysis = _parse_analysis(d.pop("analysis", UNSET))


        def _parse_balance(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        balance = _parse_balance(d.pop("balance", UNSET))


        def _parse_edge(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        edge = _parse_edge(d.pop("edge", UNSET))


        def _parse_fair_value(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        fair_value = _parse_fair_value(d.pop("fair_value", UNSET))


        def _parse_llm_duration_s(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        llm_duration_s = _parse_llm_duration_s(d.pop("llm_duration_s", UNSET))


        def _parse_market_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        market_id = _parse_market_id(d.pop("market_id", UNSET))


        def _parse_market_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        market_name = _parse_market_name(d.pop("market_name", UNSET))


        def _parse_market_price(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        market_price = _parse_market_price(d.pop("market_price", UNSET))


        def _parse_motivation(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        motivation = _parse_motivation(d.pop("motivation", UNSET))


        def _parse_no_pos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        no_pos = _parse_no_pos(d.pop("no_pos", UNSET))


        def _parse_timestamp(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        timestamp = _parse_timestamp(d.pop("timestamp", UNSET))


        def _parse_yes_pos(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        yes_pos = _parse_yes_pos(d.pop("yes_pos", UNSET))


        bot_decision_response = cls(
            article_urls=article_urls,
            id=id,
            orders=orders,
            trader_name=trader_name,
            analysis=analysis,
            balance=balance,
            edge=edge,
            fair_value=fair_value,
            llm_duration_s=llm_duration_s,
            market_id=market_id,
            market_name=market_name,
            market_price=market_price,
            motivation=motivation,
            no_pos=no_pos,
            timestamp=timestamp,
            yes_pos=yes_pos,
        )


        bot_decision_response.additional_properties = d
        return bot_decision_response

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
