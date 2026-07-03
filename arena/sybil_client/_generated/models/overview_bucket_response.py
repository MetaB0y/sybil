from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.overview_order_stats_response import OverviewOrderStatsResponse





T = TypeVar("T", bound="OverviewBucketResponse")



@_attrs_define
class OverviewBucketResponse:
    """ Per-bucket platform totals returned by `/v1/activity/overview`. B1
    populates `unique_traders` only; volume + orders join in B2 / B6 and
    remain zero until then.

        Attributes:
            orders (OverviewOrderStatsResponse | Unset):
            total_volume_nanos (int | Unset):
            total_welfare_nanos (int | Unset): Cumulative platform welfare in nanos for this bucket — sum of per-block
                `total_welfare` (each fill counted once). Signed: solver rounding can
                yield small negatives.
            unique_traders (int | Unset):
     """

    orders: OverviewOrderStatsResponse | Unset = UNSET
    total_volume_nanos: int | Unset = UNSET
    total_welfare_nanos: int | Unset = UNSET
    unique_traders: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.overview_order_stats_response import OverviewOrderStatsResponse
        orders: dict[str, Any] | Unset = UNSET
        if not isinstance(self.orders, Unset):
            orders = self.orders.to_dict()

        total_volume_nanos = self.total_volume_nanos

        total_welfare_nanos = self.total_welfare_nanos

        unique_traders = self.unique_traders


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
        })
        if orders is not UNSET:
            field_dict["orders"] = orders
        if total_volume_nanos is not UNSET:
            field_dict["total_volume_nanos"] = total_volume_nanos
        if total_welfare_nanos is not UNSET:
            field_dict["total_welfare_nanos"] = total_welfare_nanos
        if unique_traders is not UNSET:
            field_dict["unique_traders"] = unique_traders

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.overview_order_stats_response import OverviewOrderStatsResponse
        d = dict(src_dict)
        _orders = d.pop("orders", UNSET)
        orders: OverviewOrderStatsResponse | Unset
        if isinstance(_orders,  Unset):
            orders = UNSET
        else:
            orders = OverviewOrderStatsResponse.from_dict(_orders)




        total_volume_nanos = d.pop("total_volume_nanos", UNSET)

        total_welfare_nanos = d.pop("total_welfare_nanos", UNSET)

        unique_traders = d.pop("unique_traders", UNSET)

        overview_bucket_response = cls(
            orders=orders,
            total_volume_nanos=total_volume_nanos,
            total_welfare_nanos=total_welfare_nanos,
            unique_traders=unique_traders,
        )


        overview_bucket_response.additional_properties = d
        return overview_bucket_response

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
