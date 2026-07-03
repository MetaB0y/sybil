from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset







T = TypeVar("T", bound="RegisteredFeedResponse")



@_attrs_define
class RegisteredFeedResponse:
    """ Registered data feed view, returned by GET/POST /v1/feeds.

        Attributes:
            created_at_ms (int):
            feed_id (int):
            name (str):
            pubkey_hex (str):
     """

    created_at_ms: int
    feed_id: int
    name: str
    pubkey_hex: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        created_at_ms = self.created_at_ms

        feed_id = self.feed_id

        name = self.name

        pubkey_hex = self.pubkey_hex


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "created_at_ms": created_at_ms,
            "feed_id": feed_id,
            "name": name,
            "pubkey_hex": pubkey_hex,
        })

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        created_at_ms = d.pop("created_at_ms")

        feed_id = d.pop("feed_id")

        name = d.pop("name")

        pubkey_hex = d.pop("pubkey_hex")

        registered_feed_response = cls(
            created_at_ms=created_at_ms,
            feed_id=feed_id,
            name=name,
            pubkey_hex=pubkey_hex,
        )


        registered_feed_response.additional_properties = d
        return registered_feed_response

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
