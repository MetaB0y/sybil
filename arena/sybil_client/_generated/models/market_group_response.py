from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="MarketGroupResponse")



@_attrs_define
class MarketGroupResponse:
    """ 
        Attributes:
            group_id (int):
            market_ids (list[int]):
            name (str):
            creation_key (None | str | Unset):
     """

    group_id: int
    market_ids: list[int]
    name: str
    creation_key: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        group_id = self.group_id

        market_ids = self.market_ids



        name = self.name

        creation_key: None | str | Unset
        if isinstance(self.creation_key, Unset):
            creation_key = UNSET
        else:
            creation_key = self.creation_key


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "group_id": group_id,
            "market_ids": market_ids,
            "name": name,
        })
        if creation_key is not UNSET:
            field_dict["creation_key"] = creation_key

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        group_id = d.pop("group_id")

        market_ids = cast(list[int], d.pop("market_ids"))


        name = d.pop("name")

        def _parse_creation_key(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        creation_key = _parse_creation_key(d.pop("creation_key", UNSET))


        market_group_response = cls(
            group_id=group_id,
            market_ids=market_ids,
            name=name,
            creation_key=creation_key,
        )


        market_group_response.additional_properties = d
        return market_group_response

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
