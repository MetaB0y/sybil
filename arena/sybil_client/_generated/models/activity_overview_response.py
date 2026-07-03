from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.overview_bucket_response import OverviewBucketResponse





T = TypeVar("T", bound="ActivityOverviewResponse")



@_attrs_define
class ActivityOverviewResponse:
    """ Response shape for `GET /v1/activity/overview`. All-time + 24h slices.

        Attributes:
            all_time (OverviewBucketResponse): Per-bucket platform totals returned by `/v1/activity/overview`. B1
                populates `unique_traders` only; volume + orders join in B2 / B6 and
                remain zero until then.
            last_24h (OverviewBucketResponse): Per-bucket platform totals returned by `/v1/activity/overview`. B1
                populates `unique_traders` only; volume + orders join in B2 / B6 and
                remain zero until then.
     """

    all_time: OverviewBucketResponse
    last_24h: OverviewBucketResponse
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.overview_bucket_response import OverviewBucketResponse
        all_time = self.all_time.to_dict()

        last_24h = self.last_24h.to_dict()


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "all_time": all_time,
            "last_24h": last_24h,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.overview_bucket_response import OverviewBucketResponse
        d = dict(src_dict)
        all_time = OverviewBucketResponse.from_dict(d.pop("all_time"))




        last_24h = OverviewBucketResponse.from_dict(d.pop("last_24h"))




        activity_overview_response = cls(
            all_time=all_time,
            last_24h=last_24h,
        )


        activity_overview_response.additional_properties = d
        return activity_overview_response

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
