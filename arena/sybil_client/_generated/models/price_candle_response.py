from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="PriceCandleResponse")



@_attrs_define
class PriceCandleResponse:
    """ 
        Attributes:
            bucket_end_ms (int):
            bucket_start_ms (int):
            close_no_price_nanos (str): Bucket close NO price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            close_yes_price_nanos (str): Bucket close YES price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            first_height (int):
            high_no_price_nanos (str): Bucket high NO price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            high_yes_price_nanos (str): Bucket high YES price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            last_height (int):
            low_no_price_nanos (str): Bucket low NO price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            low_yes_price_nanos (str): Bucket low YES price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            open_no_price_nanos (str): Bucket open NO price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            open_yes_price_nanos (str): Bucket open YES price. Integer nanodollars; 1_000_000_000 = $1.
                Prices are per-share probabilities in [0, 1e9].
            point_count (int):
            volume_nanos (str): Bucket traded notional. Integer nanodollars; 1_000_000_000 = $1.
     """

    bucket_end_ms: int
    bucket_start_ms: int
    close_no_price_nanos: str
    close_yes_price_nanos: str
    first_height: int
    high_no_price_nanos: str
    high_yes_price_nanos: str
    last_height: int
    low_no_price_nanos: str
    low_yes_price_nanos: str
    open_no_price_nanos: str
    open_yes_price_nanos: str
    point_count: int
    volume_nanos: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        bucket_end_ms = self.bucket_end_ms

        bucket_start_ms = self.bucket_start_ms

        close_no_price_nanos = self.close_no_price_nanos

        close_yes_price_nanos = self.close_yes_price_nanos

        first_height = self.first_height

        high_no_price_nanos = self.high_no_price_nanos

        high_yes_price_nanos = self.high_yes_price_nanos

        last_height = self.last_height

        low_no_price_nanos = self.low_no_price_nanos

        low_yes_price_nanos = self.low_yes_price_nanos

        open_no_price_nanos = self.open_no_price_nanos

        open_yes_price_nanos = self.open_yes_price_nanos

        point_count = self.point_count

        volume_nanos = self.volume_nanos


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "bucket_end_ms": bucket_end_ms,
            "bucket_start_ms": bucket_start_ms,
            "close_no_price_nanos": close_no_price_nanos,
            "close_yes_price_nanos": close_yes_price_nanos,
            "first_height": first_height,
            "high_no_price_nanos": high_no_price_nanos,
            "high_yes_price_nanos": high_yes_price_nanos,
            "last_height": last_height,
            "low_no_price_nanos": low_no_price_nanos,
            "low_yes_price_nanos": low_yes_price_nanos,
            "open_no_price_nanos": open_no_price_nanos,
            "open_yes_price_nanos": open_yes_price_nanos,
            "point_count": point_count,
            "volume_nanos": volume_nanos,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        bucket_end_ms = d.pop("bucket_end_ms")

        bucket_start_ms = d.pop("bucket_start_ms")

        close_no_price_nanos = d.pop("close_no_price_nanos")

        close_yes_price_nanos = d.pop("close_yes_price_nanos")

        first_height = d.pop("first_height")

        high_no_price_nanos = d.pop("high_no_price_nanos")

        high_yes_price_nanos = d.pop("high_yes_price_nanos")

        last_height = d.pop("last_height")

        low_no_price_nanos = d.pop("low_no_price_nanos")

        low_yes_price_nanos = d.pop("low_yes_price_nanos")

        open_no_price_nanos = d.pop("open_no_price_nanos")

        open_yes_price_nanos = d.pop("open_yes_price_nanos")

        point_count = d.pop("point_count")

        volume_nanos = d.pop("volume_nanos")

        price_candle_response = cls(
            bucket_end_ms=bucket_end_ms,
            bucket_start_ms=bucket_start_ms,
            close_no_price_nanos=close_no_price_nanos,
            close_yes_price_nanos=close_yes_price_nanos,
            first_height=first_height,
            high_no_price_nanos=high_no_price_nanos,
            high_yes_price_nanos=high_yes_price_nanos,
            last_height=last_height,
            low_no_price_nanos=low_no_price_nanos,
            low_yes_price_nanos=low_yes_price_nanos,
            open_no_price_nanos=open_no_price_nanos,
            open_yes_price_nanos=open_yes_price_nanos,
            point_count=point_count,
            volume_nanos=volume_nanos,
        )


        price_candle_response.additional_properties = d
        return price_candle_response

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
