from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.api_error_response import ApiErrorResponse
from ...models.bridge_account_key_response import BridgeAccountKeyResponse
from typing import cast



def _get_kwargs(
    key_hex: str,

) -> dict[str, Any]:
    

    

    

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/bridge/accounts/by-key/{key_hex}".format(key_hex=quote(str(key_hex), safe=""),),
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | ApiErrorResponse | BridgeAccountKeyResponse | None:
    if response.status_code == 200:
        response_200 = BridgeAccountKeyResponse.from_dict(response.json())



        return response_200

    if response.status_code == 400:
        response_400 = ApiErrorResponse.from_dict(response.json())



        return response_400

    if response.status_code == 404:
        response_404 = cast(Any, None)
        return response_404

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | ApiErrorResponse | BridgeAccountKeyResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    key_hex: str,
    *,
    client: AuthenticatedClient | Client,

) -> Response[Any | ApiErrorResponse | BridgeAccountKeyResponse]:
    """ GET /v1/bridge/accounts/by-key/{key_hex}

    Args:
        key_hex (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse | BridgeAccountKeyResponse]
     """


    kwargs = _get_kwargs(
        key_hex=key_hex,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    key_hex: str,
    *,
    client: AuthenticatedClient | Client,

) -> Any | ApiErrorResponse | BridgeAccountKeyResponse | None:
    """ GET /v1/bridge/accounts/by-key/{key_hex}

    Args:
        key_hex (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse | BridgeAccountKeyResponse
     """


    return sync_detailed(
        key_hex=key_hex,
client=client,

    ).parsed

async def asyncio_detailed(
    key_hex: str,
    *,
    client: AuthenticatedClient | Client,

) -> Response[Any | ApiErrorResponse | BridgeAccountKeyResponse]:
    """ GET /v1/bridge/accounts/by-key/{key_hex}

    Args:
        key_hex (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse | BridgeAccountKeyResponse]
     """


    kwargs = _get_kwargs(
        key_hex=key_hex,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    key_hex: str,
    *,
    client: AuthenticatedClient | Client,

) -> Any | ApiErrorResponse | BridgeAccountKeyResponse | None:
    """ GET /v1/bridge/accounts/by-key/{key_hex}

    Args:
        key_hex (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse | BridgeAccountKeyResponse
     """


    return (await asyncio_detailed(
        key_hex=key_hex,
client=client,

    )).parsed
