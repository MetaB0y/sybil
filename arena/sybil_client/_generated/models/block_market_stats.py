from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset






T = TypeVar("T", bound="BlockMarketStats")



@_attrs_define
class BlockMarketStats:
    """ Nested per-market sidecar on `BlockResponse.by_market`. Grows append-only
    across steps (each new field carries `#[serde(default)]` so partial
    reverts stay clean). Volume/orders/welfare join in B2 / B6 / B7.

        Attributes:
            matched (int | Unset): Resting orders touching this market that exited the book this
                block AFTER at least one fill (B5's `has_been_matched`).
            mm_fill_notional_nanos (int | Unset): MM filled-order notional. Integer nanodollars; 1_000_000_000 = $1.
                Role-attributed notionals intentionally need not sum to unique economic
                trade volume because both matched sides appear.
            mm_matched_orders (int | Unset): Filled orders submitted through the market-maker actor epoch.
            noise_fill_notional_nanos (int | Unset): Noise filled-order notional. Integer nanodollars; 1_000_000_000 = $1.
            noise_matched_orders (int | Unset): Filled orders submitted through noise actor epochs.
            organic_fill_notional_nanos (int | Unset): Organic filled-order notional. Integer nanodollars; 1_000_000_000 =
                $1.
            organic_matched_orders (int | Unset): Filled orders from manual, LLM, or ordinary non-actor flow.
            placed (int | Unset): Non-MM admissions counted against this market in this block.
                Multi-market orders credit each active market.
            placers (int | Unset): Unique real accounts, including MM, admitted touching this market in
                the block. Multi-market orders credit each active market; the
                platform `unique_placers` scalar counts the account once.
            unmatched (int | Unset): Resting orders touching this market that exited the book this
                block WITHOUT any fill. Cancels are excluded.
            volume_nanos (int | Unset): Per-market volume contribution from this block's fills. Integer nanodollars;
                1_000_000_000 = $1. Multi-market fills credit each active market with their
                full notional; the platform `total_volume_nanos` scalar counts each fill once.
            welfare_nanos (int | Unset): Per-market welfare contribution from this block's fills (B7). Integer nanodollars;
                1_000_000_000 = $1. Multi-market fills credit each active market with their
                full welfare; the platform `total_welfare_nanos` counts each fill once.
                Encoded as signed nanos to match canonical welfare arithmetic.
     """

    matched: int | Unset = UNSET
    mm_fill_notional_nanos: int | Unset = UNSET
    mm_matched_orders: int | Unset = UNSET
    noise_fill_notional_nanos: int | Unset = UNSET
    noise_matched_orders: int | Unset = UNSET
    organic_fill_notional_nanos: int | Unset = UNSET
    organic_matched_orders: int | Unset = UNSET
    placed: int | Unset = UNSET
    placers: int | Unset = UNSET
    unmatched: int | Unset = UNSET
    volume_nanos: int | Unset = UNSET
    welfare_nanos: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        matched = self.matched

        mm_fill_notional_nanos = self.mm_fill_notional_nanos

        mm_matched_orders = self.mm_matched_orders

        noise_fill_notional_nanos = self.noise_fill_notional_nanos

        noise_matched_orders = self.noise_matched_orders

        organic_fill_notional_nanos = self.organic_fill_notional_nanos

        organic_matched_orders = self.organic_matched_orders

        placed = self.placed

        placers = self.placers

        unmatched = self.unmatched

        volume_nanos = self.volume_nanos

        welfare_nanos = self.welfare_nanos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
        })
        if matched is not UNSET:
            field_dict["matched"] = matched
        if mm_fill_notional_nanos is not UNSET:
            field_dict["mm_fill_notional_nanos"] = mm_fill_notional_nanos
        if mm_matched_orders is not UNSET:
            field_dict["mm_matched_orders"] = mm_matched_orders
        if noise_fill_notional_nanos is not UNSET:
            field_dict["noise_fill_notional_nanos"] = noise_fill_notional_nanos
        if noise_matched_orders is not UNSET:
            field_dict["noise_matched_orders"] = noise_matched_orders
        if organic_fill_notional_nanos is not UNSET:
            field_dict["organic_fill_notional_nanos"] = organic_fill_notional_nanos
        if organic_matched_orders is not UNSET:
            field_dict["organic_matched_orders"] = organic_matched_orders
        if placed is not UNSET:
            field_dict["placed"] = placed
        if placers is not UNSET:
            field_dict["placers"] = placers
        if unmatched is not UNSET:
            field_dict["unmatched"] = unmatched
        if volume_nanos is not UNSET:
            field_dict["volume_nanos"] = volume_nanos
        if welfare_nanos is not UNSET:
            field_dict["welfare_nanos"] = welfare_nanos

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        matched = d.pop("matched", UNSET)

        mm_fill_notional_nanos = d.pop("mm_fill_notional_nanos", UNSET)

        mm_matched_orders = d.pop("mm_matched_orders", UNSET)

        noise_fill_notional_nanos = d.pop("noise_fill_notional_nanos", UNSET)

        noise_matched_orders = d.pop("noise_matched_orders", UNSET)

        organic_fill_notional_nanos = d.pop("organic_fill_notional_nanos", UNSET)

        organic_matched_orders = d.pop("organic_matched_orders", UNSET)

        placed = d.pop("placed", UNSET)

        placers = d.pop("placers", UNSET)

        unmatched = d.pop("unmatched", UNSET)

        volume_nanos = d.pop("volume_nanos", UNSET)

        welfare_nanos = d.pop("welfare_nanos", UNSET)

        block_market_stats = cls(
            matched=matched,
            mm_fill_notional_nanos=mm_fill_notional_nanos,
            mm_matched_orders=mm_matched_orders,
            noise_fill_notional_nanos=noise_fill_notional_nanos,
            noise_matched_orders=noise_matched_orders,
            organic_fill_notional_nanos=organic_fill_notional_nanos,
            organic_matched_orders=organic_matched_orders,
            placed=placed,
            placers=placers,
            unmatched=unmatched,
            volume_nanos=volume_nanos,
            welfare_nanos=welfare_nanos,
        )


        block_market_stats.additional_properties = d
        return block_market_stats

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
