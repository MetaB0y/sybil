from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.price_candle_response import PriceCandleResponse





T = TypeVar("T", bound="PriceCandlesResponse")



@_attrs_define
class PriceCandlesResponse:
    """ 
        Attributes:
            candles (list[PriceCandleResponse]):
            market_id (int):
            resolution_secs (int):
            next_before_ms (int | None | Unset):
            retention_min_bucket_ms (int | None | Unset):
     """

    candles: list[PriceCandleResponse]
    market_id: int
    resolution_secs: int
    next_before_ms: int | None | Unset = UNSET
    retention_min_bucket_ms: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.price_candle_response import PriceCandleResponse
        candles = []
        for candles_item_data in self.candles:
            candles_item = candles_item_data.to_dict()
            candles.append(candles_item)



        market_id = self.market_id

        resolution_secs = self.resolution_secs

        next_before_ms: int | None | Unset
        if isinstance(self.next_before_ms, Unset):
            next_before_ms = UNSET
        else:
            next_before_ms = self.next_before_ms

        retention_min_bucket_ms: int | None | Unset
        if isinstance(self.retention_min_bucket_ms, Unset):
            retention_min_bucket_ms = UNSET
        else:
            retention_min_bucket_ms = self.retention_min_bucket_ms


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "candles": candles,
            "market_id": market_id,
            "resolution_secs": resolution_secs,
        })
        if next_before_ms is not UNSET:
            field_dict["next_before_ms"] = next_before_ms
        if retention_min_bucket_ms is not UNSET:
            field_dict["retention_min_bucket_ms"] = retention_min_bucket_ms

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.price_candle_response import PriceCandleResponse
        d = dict(src_dict)
        candles = []
        _candles = d.pop("candles")
        for candles_item_data in (_candles):
            candles_item = PriceCandleResponse.from_dict(candles_item_data)



            candles.append(candles_item)


        market_id = d.pop("market_id")

        resolution_secs = d.pop("resolution_secs")

        def _parse_next_before_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        next_before_ms = _parse_next_before_ms(d.pop("next_before_ms", UNSET))


        def _parse_retention_min_bucket_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        retention_min_bucket_ms = _parse_retention_min_bucket_ms(d.pop("retention_min_bucket_ms", UNSET))


        price_candles_response = cls(
            candles=candles,
            market_id=market_id,
            resolution_secs=resolution_secs,
            next_before_ms=next_before_ms,
            retention_min_bucket_ms=retention_min_bucket_ms,
        )


        price_candles_response.additional_properties = d
        return price_candles_response

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
