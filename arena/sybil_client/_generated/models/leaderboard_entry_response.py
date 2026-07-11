from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="LeaderboardEntryResponse")



@_attrs_define
class LeaderboardEntryResponse:
    """ 
        Attributes:
            account_id (int): Account identifier. Clients render this anonymously as `Trader #<id>`;
                display-name opt-in awaits profiles (SYB-60).
            equity_nanos (int): Current portfolio equity (balance + marked positions). Integer nanodollars; 1_000_000_000 =
                $1.
            markets_traded (int): Distinct markets with a currently open position.
            pnl_nanos (int): Net PnL over the window (realized + unrealized). Integer nanodollars; 1_000_000_000 = $1.
            rank (int): 1-based rank within the returned window.
            roi_bps (int): Return on invested capital over the window, in basis points (100 = 1%).
     """

    account_id: int
    equity_nanos: int
    markets_traded: int
    pnl_nanos: int
    rank: int
    roi_bps: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        equity_nanos = self.equity_nanos

        markets_traded = self.markets_traded

        pnl_nanos = self.pnl_nanos

        rank = self.rank

        roi_bps = self.roi_bps


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "equity_nanos": equity_nanos,
            "markets_traded": markets_traded,
            "pnl_nanos": pnl_nanos,
            "rank": rank,
            "roi_bps": roi_bps,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        equity_nanos = d.pop("equity_nanos")

        markets_traded = d.pop("markets_traded")

        pnl_nanos = d.pop("pnl_nanos")

        rank = d.pop("rank")

        roi_bps = d.pop("roi_bps")

        leaderboard_entry_response = cls(
            account_id=account_id,
            equity_nanos=equity_nanos,
            markets_traded=markets_traded,
            pnl_nanos=pnl_nanos,
            rank=rank,
            roi_bps=roi_bps,
        )


        leaderboard_entry_response.additional_properties = d
        return leaderboard_entry_response

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
