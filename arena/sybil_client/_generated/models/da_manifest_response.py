from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, BinaryIO, TextIO, TYPE_CHECKING, Generator

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

from ..types import UNSET, Unset
from typing import cast

if TYPE_CHECKING:
  from ..models.da_provider_ref_response import DaProviderRefResponse





T = TypeVar("T", bound="DaManifestResponse")



@_attrs_define
class DaManifestResponse:
    """ Typed DA manifest for retained witness payloads. SYB-120 will add encrypted
    DA fields such as ciphertext hashes and key-custody metadata here, so this
    must stay a structured DTO rather than ad-hoc JSON.

        Attributes:
            block_hash (str): Block hash bound into the state-transition public input. Hex-encoded 32-byte digest.
            da_commitment (str): DA commitment bound into the ZK public inputs and L1 RootRecord.
                Hex-encoded 32-byte digest.
            height (int):
            payload_encoding (str):
            payload_kind (str):
            payload_len (int):
            payload_root (str): Payload root = BLAKE3("sybil/da/witness-payload/v1" || len || bytes).
                Hex-encoded 32-byte digest.
            provider_refs_encoding (str):
            provider_refs_hash (str): Hash of the canonical provider-reference byte list. Hex-encoded 32-byte digest.
            public_input_hash (str): State-transition public input hash. Hex-encoded 32-byte digest.
            state_root (str): State root bound by the DA commitment. Hex-encoded 32-byte qMDB root.
            version (int):
            witness_root (str): Witness root = BLAKE3("sybil/witness" || payload bytes). Hex-encoded 32-byte digest.
            provider_refs (list[DaProviderRefResponse] | Unset):
     """

    block_hash: str
    da_commitment: str
    height: int
    payload_encoding: str
    payload_kind: str
    payload_len: int
    payload_root: str
    provider_refs_encoding: str
    provider_refs_hash: str
    public_input_hash: str
    state_root: str
    version: int
    witness_root: str
    provider_refs: list[DaProviderRefResponse] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)





    def to_dict(self) -> dict[str, Any]:
        from ..models.da_provider_ref_response import DaProviderRefResponse
        block_hash = self.block_hash

        da_commitment = self.da_commitment

        height = self.height

        payload_encoding = self.payload_encoding

        payload_kind = self.payload_kind

        payload_len = self.payload_len

        payload_root = self.payload_root

        provider_refs_encoding = self.provider_refs_encoding

        provider_refs_hash = self.provider_refs_hash

        public_input_hash = self.public_input_hash

        state_root = self.state_root

        version = self.version

        witness_root = self.witness_root

        provider_refs: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.provider_refs, Unset):
            provider_refs = []
            for provider_refs_item_data in self.provider_refs:
                provider_refs_item = provider_refs_item_data.to_dict()
                provider_refs.append(provider_refs_item)




        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({
            "block_hash": block_hash,
            "da_commitment": da_commitment,
            "height": height,
            "payload_encoding": payload_encoding,
            "payload_kind": payload_kind,
            "payload_len": payload_len,
            "payload_root": payload_root,
            "provider_refs_encoding": provider_refs_encoding,
            "provider_refs_hash": provider_refs_hash,
            "public_input_hash": public_input_hash,
            "state_root": state_root,
            "version": version,
            "witness_root": witness_root,
        })
        if provider_refs is not UNSET:
            field_dict["provider_refs"] = provider_refs

        return field_dict



    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.da_provider_ref_response import DaProviderRefResponse
        d = dict(src_dict)
        block_hash = d.pop("block_hash")

        da_commitment = d.pop("da_commitment")

        height = d.pop("height")

        payload_encoding = d.pop("payload_encoding")

        payload_kind = d.pop("payload_kind")

        payload_len = d.pop("payload_len")

        payload_root = d.pop("payload_root")

        provider_refs_encoding = d.pop("provider_refs_encoding")

        provider_refs_hash = d.pop("provider_refs_hash")

        public_input_hash = d.pop("public_input_hash")

        state_root = d.pop("state_root")

        version = d.pop("version")

        witness_root = d.pop("witness_root")

        _provider_refs = d.pop("provider_refs", UNSET)
        provider_refs: list[DaProviderRefResponse] | Unset = UNSET
        if _provider_refs is not UNSET:
            provider_refs = []
            for provider_refs_item_data in _provider_refs:
                provider_refs_item = DaProviderRefResponse.from_dict(provider_refs_item_data)



                provider_refs.append(provider_refs_item)


        da_manifest_response = cls(
            block_hash=block_hash,
            da_commitment=da_commitment,
            height=height,
            payload_encoding=payload_encoding,
            payload_kind=payload_kind,
            payload_len=payload_len,
            payload_root=payload_root,
            provider_refs_encoding=provider_refs_encoding,
            provider_refs_hash=provider_refs_hash,
            public_input_hash=public_input_hash,
            state_root=state_root,
            version=version,
            witness_root=witness_root,
            provider_refs=provider_refs,
        )


        da_manifest_response.additional_properties = d
        return da_manifest_response

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
