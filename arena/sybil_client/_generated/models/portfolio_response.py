from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.position_value_response import PositionValueResponse





T = TypeVar("T", bound="PortfolioResponse")



@_attrs_define
class PortfolioResponse:
    """ 
        Attributes:
            account_id (int):
            available_balance_nanos (int): Spendable account balance after live-order reservations. Integer
                nanodollars; 1_000_000_000 = $1.
            balance_nanos (int): Total (gross) account balance; see `available_balance_nanos` for spendable
                funds. Integer nanodollars; 1_000_000_000 = $1.
            pnl_nanos (int): Total profit and loss. Integer nanodollars; 1_000_000_000 = $1.
            portfolio_value_nanos (int): Total portfolio value. Integer nanodollars; 1_000_000_000 = $1.
            positions (list[PositionValueResponse]):
            reserved_balance_nanos (int): Balance reserved by live resting orders. Integer nanodollars;
                1_000_000_000 = $1.
            total_deposited_nanos (int): Total account deposits. Integer nanodollars; 1_000_000_000 = $1.
            total_position_value_nanos (int): Mark-to-market value of all positions. Integer nanodollars;
                1_000_000_000 = $1.
            first_deposit_ms (int | Unset): First-deposit timestamp in ms since epoch (B8). `0` for accounts
                with no recorded deposit history (FE renders as "—"). Same
                "since last restart" caveat as the other off-block aggregates
                until persistence runs in prod.
            realized_pnl_nanos (int | Unset): Accumulated realized PnL across all closed positions (C1). Integer
                nanodollars;
                1_000_000_000 = $1. Signed.
                `pnl_nanos = realized + unrealized` once both fields populate, but
                `pnl_nanos` is kept for backward compatibility with pre-C1 clients.
            total_fill_count (int | Unset): All-time fill count (B8). The bounded fill window in
                `account_fills` may cap older trades; this counter never does,
                so FE shows the real number instead of "200+".
            unrealized_pnl_nanos (int | Unset): Mark-to-market PnL on currently open positions (C1). Integer nanodollars;
                1_000_000_000 = $1. Computed as
                `sum((current_price - avg_entry) * quantity / SHARE_SCALE)` across positions.
     """

    account_id: int
    available_balance_nanos: int
    balance_nanos: int
    pnl_nanos: int
    portfolio_value_nanos: int
    positions: list[PositionValueResponse]
    reserved_balance_nanos: int
    total_deposited_nanos: int
    total_position_value_nanos: int
    first_deposit_ms: int | Unset = UNSET
    realized_pnl_nanos: int | Unset = UNSET
    total_fill_count: int | Unset = UNSET
    unrealized_pnl_nanos: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.position_value_response import PositionValueResponse
        account_id = self.account_id

        available_balance_nanos = self.available_balance_nanos

        balance_nanos = self.balance_nanos

        pnl_nanos = self.pnl_nanos

        portfolio_value_nanos = self.portfolio_value_nanos

        positions = []
        for positions_item_data in self.positions:
            positions_item = positions_item_data.to_dict()
            positions.append(positions_item)



        reserved_balance_nanos = self.reserved_balance_nanos

        total_deposited_nanos = self.total_deposited_nanos

        total_position_value_nanos = self.total_position_value_nanos

        first_deposit_ms = self.first_deposit_ms

        realized_pnl_nanos = self.realized_pnl_nanos

        total_fill_count = self.total_fill_count

        unrealized_pnl_nanos = self.unrealized_pnl_nanos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "available_balance_nanos": available_balance_nanos,
            "balance_nanos": balance_nanos,
            "pnl_nanos": pnl_nanos,
            "portfolio_value_nanos": portfolio_value_nanos,
            "positions": positions,
            "reserved_balance_nanos": reserved_balance_nanos,
            "total_deposited_nanos": total_deposited_nanos,
            "total_position_value_nanos": total_position_value_nanos,
        })
        if first_deposit_ms is not UNSET:
            field_dict["first_deposit_ms"] = first_deposit_ms
        if realized_pnl_nanos is not UNSET:
            field_dict["realized_pnl_nanos"] = realized_pnl_nanos
        if total_fill_count is not UNSET:
            field_dict["total_fill_count"] = total_fill_count
        if unrealized_pnl_nanos is not UNSET:
            field_dict["unrealized_pnl_nanos"] = unrealized_pnl_nanos

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.position_value_response import PositionValueResponse
        d = dict(src_dict)
        account_id = d.pop("account_id")

        available_balance_nanos = d.pop("available_balance_nanos")

        balance_nanos = d.pop("balance_nanos")

        pnl_nanos = d.pop("pnl_nanos")

        portfolio_value_nanos = d.pop("portfolio_value_nanos")

        positions = []
        _positions = d.pop("positions")
        for positions_item_data in (_positions):
            positions_item = PositionValueResponse.from_dict(positions_item_data)



            positions.append(positions_item)


        reserved_balance_nanos = d.pop("reserved_balance_nanos")

        total_deposited_nanos = d.pop("total_deposited_nanos")

        total_position_value_nanos = d.pop("total_position_value_nanos")

        first_deposit_ms = d.pop("first_deposit_ms", UNSET)

        realized_pnl_nanos = d.pop("realized_pnl_nanos", UNSET)

        total_fill_count = d.pop("total_fill_count", UNSET)

        unrealized_pnl_nanos = d.pop("unrealized_pnl_nanos", UNSET)

        portfolio_response = cls(
            account_id=account_id,
            available_balance_nanos=available_balance_nanos,
            balance_nanos=balance_nanos,
            pnl_nanos=pnl_nanos,
            portfolio_value_nanos=portfolio_value_nanos,
            positions=positions,
            reserved_balance_nanos=reserved_balance_nanos,
            total_deposited_nanos=total_deposited_nanos,
            total_position_value_nanos=total_position_value_nanos,
            first_deposit_ms=first_deposit_ms,
            realized_pnl_nanos=realized_pnl_nanos,
            total_fill_count=total_fill_count,
            unrealized_pnl_nanos=unrealized_pnl_nanos,
        )


        portfolio_response.additional_properties = d
        return portfolio_response

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
