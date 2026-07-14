from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="BotSummaryResponse")



@_attrs_define
class BotSummaryResponse:
    """ 
        Attributes:
            active (bool): Member of the most recent non-stale Arena runtime cohort.
            decision_count (int):
            scored (bool): Eligible for public competition totals within the active runtime.
            trader_name (str):
            account_id (int | None | Unset):
            avg_edge (float | None | Unset):
            latest_balance (float | None | Unset):
            latest_edge (float | None | Unset):
            latest_fair_value (float | None | Unset):
            latest_market_id (int | None | Unset):
            latest_market_name (None | str | Unset):
            latest_market_price (float | None | Unset):
            latest_timestamp (None | str | Unset):
            participant_kind (None | str | Unset):
            pnl (float | None | Unset):
            portfolio_value (float | None | Unset):
            role (None | str | Unset): Runtime role such as competitor, load, or noise.
            snapshot_timestamp (None | str | Unset):
            total_fills (int | None | Unset):
            total_orders (int | None | Unset):
     """

    active: bool
    decision_count: int
    scored: bool
    trader_name: str
    account_id: int | None | Unset = UNSET
    avg_edge: float | None | Unset = UNSET
    latest_balance: float | None | Unset = UNSET
    latest_edge: float | None | Unset = UNSET
    latest_fair_value: float | None | Unset = UNSET
    latest_market_id: int | None | Unset = UNSET
    latest_market_name: None | str | Unset = UNSET
    latest_market_price: float | None | Unset = UNSET
    latest_timestamp: None | str | Unset = UNSET
    participant_kind: None | str | Unset = UNSET
    pnl: float | None | Unset = UNSET
    portfolio_value: float | None | Unset = UNSET
    role: None | str | Unset = UNSET
    snapshot_timestamp: None | str | Unset = UNSET
    total_fills: int | None | Unset = UNSET
    total_orders: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        active = self.active

        decision_count = self.decision_count

        scored = self.scored

        trader_name = self.trader_name

        account_id: int | None | Unset
        if isinstance(self.account_id, Unset):
            account_id = UNSET
        else:
            account_id = self.account_id

        avg_edge: float | None | Unset
        if isinstance(self.avg_edge, Unset):
            avg_edge = UNSET
        else:
            avg_edge = self.avg_edge

        latest_balance: float | None | Unset
        if isinstance(self.latest_balance, Unset):
            latest_balance = UNSET
        else:
            latest_balance = self.latest_balance

        latest_edge: float | None | Unset
        if isinstance(self.latest_edge, Unset):
            latest_edge = UNSET
        else:
            latest_edge = self.latest_edge

        latest_fair_value: float | None | Unset
        if isinstance(self.latest_fair_value, Unset):
            latest_fair_value = UNSET
        else:
            latest_fair_value = self.latest_fair_value

        latest_market_id: int | None | Unset
        if isinstance(self.latest_market_id, Unset):
            latest_market_id = UNSET
        else:
            latest_market_id = self.latest_market_id

        latest_market_name: None | str | Unset
        if isinstance(self.latest_market_name, Unset):
            latest_market_name = UNSET
        else:
            latest_market_name = self.latest_market_name

        latest_market_price: float | None | Unset
        if isinstance(self.latest_market_price, Unset):
            latest_market_price = UNSET
        else:
            latest_market_price = self.latest_market_price

        latest_timestamp: None | str | Unset
        if isinstance(self.latest_timestamp, Unset):
            latest_timestamp = UNSET
        else:
            latest_timestamp = self.latest_timestamp

        participant_kind: None | str | Unset
        if isinstance(self.participant_kind, Unset):
            participant_kind = UNSET
        else:
            participant_kind = self.participant_kind

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

        role: None | str | Unset
        if isinstance(self.role, Unset):
            role = UNSET
        else:
            role = self.role

        snapshot_timestamp: None | str | Unset
        if isinstance(self.snapshot_timestamp, Unset):
            snapshot_timestamp = UNSET
        else:
            snapshot_timestamp = self.snapshot_timestamp

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
            "active": active,
            "decision_count": decision_count,
            "scored": scored,
            "trader_name": trader_name,
        })
        if account_id is not UNSET:
            field_dict["account_id"] = account_id
        if avg_edge is not UNSET:
            field_dict["avg_edge"] = avg_edge
        if latest_balance is not UNSET:
            field_dict["latest_balance"] = latest_balance
        if latest_edge is not UNSET:
            field_dict["latest_edge"] = latest_edge
        if latest_fair_value is not UNSET:
            field_dict["latest_fair_value"] = latest_fair_value
        if latest_market_id is not UNSET:
            field_dict["latest_market_id"] = latest_market_id
        if latest_market_name is not UNSET:
            field_dict["latest_market_name"] = latest_market_name
        if latest_market_price is not UNSET:
            field_dict["latest_market_price"] = latest_market_price
        if latest_timestamp is not UNSET:
            field_dict["latest_timestamp"] = latest_timestamp
        if participant_kind is not UNSET:
            field_dict["participant_kind"] = participant_kind
        if pnl is not UNSET:
            field_dict["pnl"] = pnl
        if portfolio_value is not UNSET:
            field_dict["portfolio_value"] = portfolio_value
        if role is not UNSET:
            field_dict["role"] = role
        if snapshot_timestamp is not UNSET:
            field_dict["snapshot_timestamp"] = snapshot_timestamp
        if total_fills is not UNSET:
            field_dict["total_fills"] = total_fills
        if total_orders is not UNSET:
            field_dict["total_orders"] = total_orders

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        active = d.pop("active")

        decision_count = d.pop("decision_count")

        scored = d.pop("scored")

        trader_name = d.pop("trader_name")

        def _parse_account_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        account_id = _parse_account_id(d.pop("account_id", UNSET))


        def _parse_avg_edge(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        avg_edge = _parse_avg_edge(d.pop("avg_edge", UNSET))


        def _parse_latest_balance(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        latest_balance = _parse_latest_balance(d.pop("latest_balance", UNSET))


        def _parse_latest_edge(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        latest_edge = _parse_latest_edge(d.pop("latest_edge", UNSET))


        def _parse_latest_fair_value(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        latest_fair_value = _parse_latest_fair_value(d.pop("latest_fair_value", UNSET))


        def _parse_latest_market_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        latest_market_id = _parse_latest_market_id(d.pop("latest_market_id", UNSET))


        def _parse_latest_market_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        latest_market_name = _parse_latest_market_name(d.pop("latest_market_name", UNSET))


        def _parse_latest_market_price(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        latest_market_price = _parse_latest_market_price(d.pop("latest_market_price", UNSET))


        def _parse_latest_timestamp(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        latest_timestamp = _parse_latest_timestamp(d.pop("latest_timestamp", UNSET))


        def _parse_participant_kind(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        participant_kind = _parse_participant_kind(d.pop("participant_kind", UNSET))


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


        def _parse_role(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        role = _parse_role(d.pop("role", UNSET))


        def _parse_snapshot_timestamp(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        snapshot_timestamp = _parse_snapshot_timestamp(d.pop("snapshot_timestamp", UNSET))


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


        bot_summary_response = cls(
            active=active,
            decision_count=decision_count,
            scored=scored,
            trader_name=trader_name,
            account_id=account_id,
            avg_edge=avg_edge,
            latest_balance=latest_balance,
            latest_edge=latest_edge,
            latest_fair_value=latest_fair_value,
            latest_market_id=latest_market_id,
            latest_market_name=latest_market_name,
            latest_market_price=latest_market_price,
            latest_timestamp=latest_timestamp,
            participant_kind=participant_kind,
            pnl=pnl,
            portfolio_value=portfolio_value,
            role=role,
            snapshot_timestamp=snapshot_timestamp,
            total_fills=total_fills,
            total_orders=total_orders,
        )


        bot_summary_response.additional_properties = d
        return bot_summary_response

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
