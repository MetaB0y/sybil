from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.state_proof_response import StateProofResponse
from typing import cast



def _get_kwargs(
    leaf_key_hex: str,

) -> dict[str, Any]:
    

    

    

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/proofs/state/{leaf_key_hex}".format(leaf_key_hex=quote(str(leaf_key_hex), safe=""),),
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | StateProofResponse | None:
    if response.status_code == 200:
        response_200 = StateProofResponse.from_dict(response.json())



        return response_200

    if response.status_code == 400:
        response_400 = cast(Any, None)
        return response_400

    if response.status_code == 404:
        response_404 = cast(Any, None)
        return response_404

    if response.status_code == 503:
        response_503 = cast(Any, None)
        return response_503

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | StateProofResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    leaf_key_hex: str,
    *,
    client: AuthenticatedClient | Client,

) -> Response[Any | StateProofResponse]:
    """ GET /v1/proofs/state/{leaf_key_hex}

    Args:
        leaf_key_hex (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | StateProofResponse]
     """


    kwargs = _get_kwargs(
        leaf_key_hex=leaf_key_hex,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    leaf_key_hex: str,
    *,
    client: AuthenticatedClient | Client,

) -> Any | StateProofResponse | None:
    """ GET /v1/proofs/state/{leaf_key_hex}

    Args:
        leaf_key_hex (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | StateProofResponse
     """


    return sync_detailed(
        leaf_key_hex=leaf_key_hex,
client=client,

    ).parsed

async def asyncio_detailed(
    leaf_key_hex: str,
    *,
    client: AuthenticatedClient | Client,

) -> Response[Any | StateProofResponse]:
    """ GET /v1/proofs/state/{leaf_key_hex}

    Args:
        leaf_key_hex (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | StateProofResponse]
     """


    kwargs = _get_kwargs(
        leaf_key_hex=leaf_key_hex,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    leaf_key_hex: str,
    *,
    client: AuthenticatedClient | Client,

) -> Any | StateProofResponse | None:
    """ GET /v1/proofs/state/{leaf_key_hex}

    Args:
        leaf_key_hex (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | StateProofResponse
     """


    return (await asyncio_detailed(
        leaf_key_hex=leaf_key_hex,
client=client,

    )).parsed
