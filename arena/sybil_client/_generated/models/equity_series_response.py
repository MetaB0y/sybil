from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from typing import cast

if TYPE_CHECKING:
  from ..models.equity_point_response import EquityPointResponse





T = TypeVar("T", bound="EquitySeriesResponse")



@_attrs_define
class EquitySeriesResponse:
    """ 
        Attributes:
            account_id (int):
            points (list[EquityPointResponse]):
     """

    account_id: int
    points: list[EquityPointResponse]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.equity_point_response import EquityPointResponse
        account_id = self.account_id

        points = []
        for points_item_data in self.points:
            points_item = points_item_data.to_dict()
            points.append(points_item)




        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "account_id": account_id,
            "points": points,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.equity_point_response import EquityPointResponse
        d = dict(src_dict)
        account_id = d.pop("account_id")

        points = []
        _points = d.pop("points")
        for points_item_data in (_points):
            points_item = EquityPointResponse.from_dict(points_item_data)



            points.append(points_item)


        equity_series_response = cls(
            account_id=account_id,
            points=points,
        )


        equity_series_response.additional_properties = d
        return equity_series_response

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
