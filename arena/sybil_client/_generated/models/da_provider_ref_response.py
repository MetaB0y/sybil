from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast






T = TypeVar("T", bound="DaProviderRefResponse")



@_attrs_define
class DaProviderRefResponse:
    """ 
        Attributes:
            bytes_ (str): Hex-encoded canonical provider-reference bytes, 0x-prefixed.
            encoding (str):
            kind (str):
            payload_len (int | None | Unset):
            payload_root (None | str | Unset): Payload root repeated when the provider ref is content-addressed.
                Hex-encoded 32-byte digest.
            uri (None | str | Unset):
     """

    bytes_: str
    encoding: str
    kind: str
    payload_len: int | None | Unset = UNSET
    payload_root: None | str | Unset = UNSET
    uri: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        bytes_ = self.bytes_

        encoding = self.encoding

        kind = self.kind

        payload_len: int | None | Unset
        if isinstance(self.payload_len, Unset):
            payload_len = UNSET
        else:
            payload_len = self.payload_len

        payload_root: None | str | Unset
        if isinstance(self.payload_root, Unset):
            payload_root = UNSET
        else:
            payload_root = self.payload_root

        uri: None | str | Unset
        if isinstance(self.uri, Unset):
            uri = UNSET
        else:
            uri = self.uri


        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "bytes": bytes_,
            "encoding": encoding,
            "kind": kind,
        })
        if payload_len is not UNSET:
            field_dict["payload_len"] = payload_len
        if payload_root is not UNSET:
            field_dict["payload_root"] = payload_root
        if uri is not UNSET:
            field_dict["uri"] = uri

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        bytes_ = d.pop("bytes")

        encoding = d.pop("encoding")

        kind = d.pop("kind")

        def _parse_payload_len(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        payload_len = _parse_payload_len(d.pop("payload_len", UNSET))


        def _parse_payload_root(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        payload_root = _parse_payload_root(d.pop("payload_root", UNSET))


        def _parse_uri(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        uri = _parse_uri(d.pop("uri", UNSET))


        da_provider_ref_response = cls(
            bytes_=bytes_,
            encoding=encoding,
            kind=kind,
            payload_len=payload_len,
            payload_root=payload_root,
            uri=uri,
        )


        da_provider_ref_response.additional_properties = d
        return da_provider_ref_response

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
