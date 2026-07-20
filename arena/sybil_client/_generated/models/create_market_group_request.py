from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="CreateMarketGroupRequest")



@_attrs_define
class CreateMarketGroupRequest:
    """ 
        Attributes:
            market_ids (list[int]): Market IDs in the group.
            name (str): Name for the group of mutually exclusive markets. Example: 2024 Election.
            creation_key (None | str | Unset): Optional stable operator identity. Exact retries return the original
                group; reuse with different creation fields is rejected.
     """

    market_ids: list[int]
    name: str
    creation_key: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
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
            "market_ids": market_ids,
            "name": name,
        })
        if creation_key is not UNSET:
            field_dict["creation_key"] = creation_key

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        market_ids = cast(list[int], d.pop("market_ids"))


        name = d.pop("name")

        def _parse_creation_key(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        creation_key = _parse_creation_key(d.pop("creation_key", UNSET))


        create_market_group_request = cls(
            market_ids=market_ids,
            name=name,
            creation_key=creation_key,
        )


        create_market_group_request.additional_properties = d
        return create_market_group_request

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
