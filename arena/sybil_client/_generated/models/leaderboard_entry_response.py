from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="LeaderboardEntryResponse")



@_attrs_define
class LeaderboardEntryResponse:
    """ 
        Attributes:
            account_id (int): Account identifier. Clients render this anonymously as `Trader #<id>`;
                display-name opt-in awaits profiles (SYB-60).
            display_name (str): Signed opt-in public profile name. Its presence is the publication
                consent boundary for this entire financial row.
            equity_nanos (str): Current portfolio equity (balance + marked positions). Integer nanodollars; 1_000_000_000 =
                $1.
            markets_traded (int): Distinct markets with a currently open position.
            pnl_nanos (str): Net PnL over the window (realized + unrealized). Integer nanodollars; 1_000_000_000 = $1.
            rank (int): 1-based rank within the returned window.
            roi_bps (int): Return on invested capital over the window, in basis points (100 = 1%).
            avatar_seed (None | str | Unset):
     """

    account_id: int
    display_name: str
    equity_nanos: str
    markets_traded: int
    pnl_nanos: str
    rank: int
    roi_bps: int
    avatar_seed: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        account_id = self.account_id

        display_name = self.display_name

        equity_nanos = self.equity_nanos

        markets_traded = self.markets_traded

        pnl_nanos = self.pnl_nanos

        rank = self.rank

        roi_bps = self.roi_bps

        avatar_seed: None | str | Unset
        if isinstance(self.avatar_seed, Unset):
            avatar_seed = UNSET
        else:
            avatar_seed = self.avatar_seed


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "display_name": display_name,
            "equity_nanos": equity_nanos,
            "markets_traded": markets_traded,
            "pnl_nanos": pnl_nanos,
            "rank": rank,
            "roi_bps": roi_bps,
        })
        if avatar_seed is not UNSET:
            field_dict["avatar_seed"] = avatar_seed

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        account_id = d.pop("account_id")

        display_name = d.pop("display_name")

        equity_nanos = d.pop("equity_nanos")

        markets_traded = d.pop("markets_traded")

        pnl_nanos = d.pop("pnl_nanos")

        rank = d.pop("rank")

        roi_bps = d.pop("roi_bps")

        def _parse_avatar_seed(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        avatar_seed = _parse_avatar_seed(d.pop("avatar_seed", UNSET))


        leaderboard_entry_response = cls(
            account_id=account_id,
            display_name=display_name,
            equity_nanos=equity_nanos,
            markets_traded=markets_traded,
            pnl_nanos=pnl_nanos,
            rank=rank,
            roi_bps=roi_bps,
            avatar_seed=avatar_seed,
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
