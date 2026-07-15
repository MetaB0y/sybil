from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.public_block_response_by_market import PublicBlockResponseByMarket
  from ..models.public_block_response_clearing_prices_nanos import PublicBlockResponseClearingPricesNanos
  from ..models.public_bridge_block_response import PublicBridgeBlockResponse





T = TypeVar("T", bound="PublicBlockResponse")



@_attrs_define
class PublicBlockResponse:
    """ Privacy-preserving projection of a committed block for public REST and
    streaming clients. Account-attributed fills, rejections, system events,
    bridge leaves, and order-lifecycle rows deliberately do not exist on this
    type; canonical full blocks remain available only to authenticated service
    consumers.

        Attributes:
            bridge (PublicBridgeBlockResponse):
            events_root (str):
            fill_count (int):
            height (int):
            order_count (int):
            orders_filled (int):
            parent_hash (str):
            rejection_count (int): Number of rejected orders without identities, order ids, or reasons.
            state_root (str): Post-block state root. Hex-encoded 32-byte qMDB root.
            timestamp_ms (int):
            total_volume_nanos (int): Total traded notional in the block. Integer nanodollars;
                1_000_000_000 = $1.
            total_welfare_nanos (int): Total solver welfare in the block. Integer nanodollars;
                1_000_000_000 = $1. Signed: solver rounding can yield small negatives.
            by_market (PublicBlockResponseByMarket | Unset):
            clearing_prices_nanos (PublicBlockResponseClearingPricesNanos | Unset): Clearing price vectors by market/group.
                Integer nanodollars;
                1_000_000_000 = $1. Prices are per-share probabilities in [0, 1e9].
            resolved_market_ids (list[int] | Unset): Market ids resolved in this block. The account-bearing affected-account
                list from the canonical event is intentionally omitted.
            unique_placers (int | Unset): Unique non-MM accounts admitted into this block. This is an aggregate,
                never an account identifier list.
     """

    bridge: PublicBridgeBlockResponse
    events_root: str
    fill_count: int
    height: int
    order_count: int
    orders_filled: int
    parent_hash: str
    rejection_count: int
    state_root: str
    timestamp_ms: int
    total_volume_nanos: int
    total_welfare_nanos: int
    by_market: PublicBlockResponseByMarket | Unset = UNSET
    clearing_prices_nanos: PublicBlockResponseClearingPricesNanos | Unset = UNSET
    resolved_market_ids: list[int] | Unset = UNSET
    unique_placers: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.public_block_response_by_market import PublicBlockResponseByMarket
        from ..models.public_block_response_clearing_prices_nanos import PublicBlockResponseClearingPricesNanos
        from ..models.public_bridge_block_response import PublicBridgeBlockResponse
        bridge = self.bridge.to_dict()

        events_root = self.events_root

        fill_count = self.fill_count

        height = self.height

        order_count = self.order_count

        orders_filled = self.orders_filled

        parent_hash = self.parent_hash

        rejection_count = self.rejection_count

        state_root = self.state_root

        timestamp_ms = self.timestamp_ms

        total_volume_nanos = self.total_volume_nanos

        total_welfare_nanos = self.total_welfare_nanos

        by_market: dict[str, Any] | Unset = UNSET
        if not isinstance(self.by_market, Unset):
            by_market = self.by_market.to_dict()

        clearing_prices_nanos: dict[str, Any] | Unset = UNSET
        if not isinstance(self.clearing_prices_nanos, Unset):
            clearing_prices_nanos = self.clearing_prices_nanos.to_dict()

        resolved_market_ids: list[int] | Unset = UNSET
        if not isinstance(self.resolved_market_ids, Unset):
            resolved_market_ids = self.resolved_market_ids



        unique_placers = self.unique_placers


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "bridge": bridge,
            "events_root": events_root,
            "fill_count": fill_count,
            "height": height,
            "order_count": order_count,
            "orders_filled": orders_filled,
            "parent_hash": parent_hash,
            "rejection_count": rejection_count,
            "state_root": state_root,
            "timestamp_ms": timestamp_ms,
            "total_volume_nanos": total_volume_nanos,
            "total_welfare_nanos": total_welfare_nanos,
        })
        if by_market is not UNSET:
            field_dict["by_market"] = by_market
        if clearing_prices_nanos is not UNSET:
            field_dict["clearing_prices_nanos"] = clearing_prices_nanos
        if resolved_market_ids is not UNSET:
            field_dict["resolved_market_ids"] = resolved_market_ids
        if unique_placers is not UNSET:
            field_dict["unique_placers"] = unique_placers

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.public_block_response_by_market import PublicBlockResponseByMarket
        from ..models.public_block_response_clearing_prices_nanos import PublicBlockResponseClearingPricesNanos
        from ..models.public_bridge_block_response import PublicBridgeBlockResponse
        d = dict(src_dict)
        bridge = PublicBridgeBlockResponse.from_dict(d.pop("bridge"))




        events_root = d.pop("events_root")

        fill_count = d.pop("fill_count")

        height = d.pop("height")

        order_count = d.pop("order_count")

        orders_filled = d.pop("orders_filled")

        parent_hash = d.pop("parent_hash")

        rejection_count = d.pop("rejection_count")

        state_root = d.pop("state_root")

        timestamp_ms = d.pop("timestamp_ms")

        total_volume_nanos = d.pop("total_volume_nanos")

        total_welfare_nanos = d.pop("total_welfare_nanos")

        _by_market = d.pop("by_market", UNSET)
        by_market: PublicBlockResponseByMarket | Unset
        if isinstance(_by_market,  Unset):
            by_market = UNSET
        else:
            by_market = PublicBlockResponseByMarket.from_dict(_by_market)




        _clearing_prices_nanos = d.pop("clearing_prices_nanos", UNSET)
        clearing_prices_nanos: PublicBlockResponseClearingPricesNanos | Unset
        if isinstance(_clearing_prices_nanos,  Unset):
            clearing_prices_nanos = UNSET
        else:
            clearing_prices_nanos = PublicBlockResponseClearingPricesNanos.from_dict(_clearing_prices_nanos)




        resolved_market_ids = cast(list[int], d.pop("resolved_market_ids", UNSET))


        unique_placers = d.pop("unique_placers", UNSET)

        public_block_response = cls(
            bridge=bridge,
            events_root=events_root,
            fill_count=fill_count,
            height=height,
            order_count=order_count,
            orders_filled=orders_filled,
            parent_hash=parent_hash,
            rejection_count=rejection_count,
            state_root=state_root,
            timestamp_ms=timestamp_ms,
            total_volume_nanos=total_volume_nanos,
            total_welfare_nanos=total_welfare_nanos,
            by_market=by_market,
            clearing_prices_nanos=clearing_prices_nanos,
            resolved_market_ids=resolved_market_ids,
            unique_placers=unique_placers,
        )


        public_block_response.additional_properties = d
        return public_block_response

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
