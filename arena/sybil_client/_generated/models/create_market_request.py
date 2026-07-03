from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="CreateMarketRequest")



@_attrs_define
class CreateMarketRequest:
    """ 
        Attributes:
            name (str): Name of the binary market. Example: Will it rain tomorrow?.
            category (None | str | Unset): Optional category (e.g., "sports", "politics", "crypto").
            description (None | str | Unset): Optional description of the market.
            expiry_timestamp_ms (int | None | Unset): Optional expiry timestamp in ms (0 = no expiry).
            resolution_criteria (None | str | Unset): Optional resolution criteria.
            resolution_template (None | str | Unset): Resolution template id to use for this market (e.g. "admin_immediate",
                "polymarket_mirror"). `None` -> `admin_immediate`.
            tags (list[str] | None | Unset): Optional tags for discovery.
     """

    name: str
    category: None | str | Unset = UNSET
    description: None | str | Unset = UNSET
    expiry_timestamp_ms: int | None | Unset = UNSET
    resolution_criteria: None | str | Unset = UNSET
    resolution_template: None | str | Unset = UNSET
    tags: list[str] | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        name = self.name

        category: None | str | Unset
        if isinstance(self.category, Unset):
            category = UNSET
        else:
            category = self.category

        description: None | str | Unset
        if isinstance(self.description, Unset):
            description = UNSET
        else:
            description = self.description

        expiry_timestamp_ms: int | None | Unset
        if isinstance(self.expiry_timestamp_ms, Unset):
            expiry_timestamp_ms = UNSET
        else:
            expiry_timestamp_ms = self.expiry_timestamp_ms

        resolution_criteria: None | str | Unset
        if isinstance(self.resolution_criteria, Unset):
            resolution_criteria = UNSET
        else:
            resolution_criteria = self.resolution_criteria

        resolution_template: None | str | Unset
        if isinstance(self.resolution_template, Unset):
            resolution_template = UNSET
        else:
            resolution_template = self.resolution_template

        tags: list[str] | None | Unset
        if isinstance(self.tags, Unset):
            tags = UNSET
        elif isinstance(self.tags, list):
            tags = self.tags


        else:
            tags = self.tags


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "name": name,
        })
        if category is not UNSET:
            field_dict["category"] = category
        if description is not UNSET:
            field_dict["description"] = description
        if expiry_timestamp_ms is not UNSET:
            field_dict["expiry_timestamp_ms"] = expiry_timestamp_ms
        if resolution_criteria is not UNSET:
            field_dict["resolution_criteria"] = resolution_criteria
        if resolution_template is not UNSET:
            field_dict["resolution_template"] = resolution_template
        if tags is not UNSET:
            field_dict["tags"] = tags

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        name = d.pop("name")

        def _parse_category(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        category = _parse_category(d.pop("category", UNSET))


        def _parse_description(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        description = _parse_description(d.pop("description", UNSET))


        def _parse_expiry_timestamp_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        expiry_timestamp_ms = _parse_expiry_timestamp_ms(d.pop("expiry_timestamp_ms", UNSET))


        def _parse_resolution_criteria(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        resolution_criteria = _parse_resolution_criteria(d.pop("resolution_criteria", UNSET))


        def _parse_resolution_template(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        resolution_template = _parse_resolution_template(d.pop("resolution_template", UNSET))


        def _parse_tags(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                tags_type_0 = cast(list[str], data)

                return tags_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        tags = _parse_tags(d.pop("tags", UNSET))


        create_market_request = cls(
            name=name,
            category=category,
            description=description,
            expiry_timestamp_ms=expiry_timestamp_ms,
            resolution_criteria=resolution_criteria,
            resolution_template=resolution_template,
            tags=tags,
        )


        create_market_request.additional_properties = d
        return create_market_request

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
