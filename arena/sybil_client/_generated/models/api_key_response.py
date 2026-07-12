from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="ApiKeyResponse")



@_attrs_define
class ApiKeyResponse:
    """ A read-scoped bearer API key's metadata (never the token or its hash).

        Attributes:
            created_at_ms (int):
            id (int): Stable id used to reference this key for revocation.
            label (None | str | Unset):
            revoked_at_ms (int | None | Unset): Revocation time in Unix milliseconds, if revoked.
     """

    created_at_ms: int
    id: int
    label: None | str | Unset = UNSET
    revoked_at_ms: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        created_at_ms = self.created_at_ms

        id = self.id

        label: None | str | Unset
        if isinstance(self.label, Unset):
            label = UNSET
        else:
            label = self.label

        revoked_at_ms: int | None | Unset
        if isinstance(self.revoked_at_ms, Unset):
            revoked_at_ms = UNSET
        else:
            revoked_at_ms = self.revoked_at_ms


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "created_at_ms": created_at_ms,
            "id": id,
        })
        if label is not UNSET:
            field_dict["label"] = label
        if revoked_at_ms is not UNSET:
            field_dict["revoked_at_ms"] = revoked_at_ms

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        created_at_ms = d.pop("created_at_ms")

        id = d.pop("id")

        def _parse_label(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        label = _parse_label(d.pop("label", UNSET))


        def _parse_revoked_at_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        revoked_at_ms = _parse_revoked_at_ms(d.pop("revoked_at_ms", UNSET))


        api_key_response = cls(
            created_at_ms=created_at_ms,
            id=id,
            label=label,
            revoked_at_ms=revoked_at_ms,
        )


        api_key_response.additional_properties = d
        return api_key_response

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
